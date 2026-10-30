"""Exercise aips-core's production (Keycloak/OIDC) token validation without a Keycloak server.

Serves a JWKS at the Keycloak path, mints RS256 tokens with a local key, and checks that the
service accepts only correctly signed, unexpired tokens for the right issuer and audience.
Usage: python3 tests/security/oidc_validation.py <core-binary>
"""
import base64, json, os, subprocess, sys, threading, time, urllib.request, urllib.error
from http.server import BaseHTTPRequestHandler, HTTPServer
import jwt
from cryptography.hazmat.primitives.asymmetric import rsa

ISS_PORT, CORE_PORT = 9871, 8095
ISSUER = f"http://127.0.0.1:{ISS_PORT}/realms/aips"
key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
other = rsa.generate_private_key(public_exponent=65537, key_size=2048)
nums = key.public_key().public_numbers()
b64 = lambda n: base64.urlsafe_b64encode(n.to_bytes((n.bit_length() + 7) // 8, "big")).rstrip(b"=").decode()
JWKS = {"keys": [{"kty": "RSA", "kid": "k1", "use": "sig", "alg": "RS256", "n": b64(nums.n), "e": b64(nums.e)}]}

class H(BaseHTTPRequestHandler):
    def do_GET(self):
        body = json.dumps(JWKS).encode() if self.path.endswith("/protocol/openid-connect/certs") else b"{}"
        self.send_response(200); self.send_header("content-type", "application/json"); self.end_headers(); self.wfile.write(body)
    def log_message(self, *a): pass

threading.Thread(target=HTTPServer(("127.0.0.1", ISS_PORT), H).serve_forever, daemon=True).start()
env = dict(os.environ, AIPS_AUTH_MODE="keycloak", AIPS_OIDC_ISSUER=ISSUER, AIPS_OIDC_AUDIENCE="aips-api",
           AIPS_CORE_ADDR=f"127.0.0.1:{CORE_PORT}", AIPS_ENV="production")
env.pop("DATABASE_URL", None)
proc = subprocess.Popen([sys.argv[1]], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def token(signer=key, kid="k1", iss=ISSUER, aud="aips-api", exp=300, roles=("engineer",), alg="RS256"):
    now = int(time.time())
    claims = {"sub": "u1", "preferred_username": "test.user", "iss": iss, "aud": aud, "iat": now, "exp": now + exp,
              "realm_access": {"roles": list(roles)}}
    return jwt.encode(claims, signer, algorithm=alg, headers={"kid": kid})

def get(path, tok=None):
    req = urllib.request.Request(f"http://127.0.0.1:{CORE_PORT}{path}")
    if tok: req.add_header("authorization", f"Bearer {tok}")
    try: return urllib.request.urlopen(req, timeout=5).status
    except urllib.error.HTTPError as e: return e.code

for _ in range(100):
    try:
        if get("/readyz") == 200: break
    except Exception: pass
    time.sleep(0.2)

good = token()
parts = good.split(".")
tampered = ".".join([parts[0], base64.urlsafe_b64encode(json.dumps({"sub": "x", "iss": ISSUER, "aud": "aips-api", "exp": int(time.time()) + 300, "realm_access": {"roles": ["admin"]}}).encode()).rstrip(b"=").decode(), parts[2]])
none_alg = base64.urlsafe_b64encode(b'{"alg":"none","kid":"k1"}').rstrip(b"=").decode() + "." + parts[1] + "."
cases = [
    ("valid token, engineer reads fleet", get("/api/fleet", good), 200),
    ("dev headers ignored in keycloak mode", get("/api/fleet"), 401),
    ("expired token", get("/api/fleet", token(exp=-120)), 401),
    ("wrong audience", get("/api/fleet", token(aud="other-api")), 401),
    ("wrong issuer", get("/api/fleet", token(iss="http://evil.example/realms/aips")), 401),
    ("signed by an unknown key", get("/api/fleet", token(signer=other)), 401),
    ("unknown key id", get("/api/fleet", token(kid="k9")), 401),
    ("tampered payload (role escalation)", get("/api/fleet", tampered), 401),
    ("alg=none token", get("/api/fleet", none_alg), 401),
    ("HS256 token using public key material", get("/api/fleet", jwt.encode({"sub": "x"}, "secret", algorithm="HS256", headers={"kid": "k1"})), 401),
    ("valid token, insufficient role", get("/api/audit", token(roles=("flight_ops",))), 403),
    ("valid token, unknown roles filtered out", get("/api/fleet", token(roles=("superadmin",))), 403),
]
proc.terminate()
ok = 0
for name, got, want in cases:
    status = "PASS" if got == want else "FAIL"
    ok += got == want
    print(f"  {status}  {name}: HTTP {got} (expected {want})")
print(f"{ok}/{len(cases)} passed")
sys.exit(0 if ok == len(cases) else 1)
