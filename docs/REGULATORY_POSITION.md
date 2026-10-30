# AIPS Phase 0: Regulatory position paper

Status: draft. It needs review by aviation counsel and, ideally, an informal opinion from a
competent authority before any design-partner pilot. Nothing here is a legal opinion.

## Intended function

AIPS is a ground-based advisory system. It collects aircraft health, maintenance and flight data
that operators already hold, finds patterns across flights, and presents evidence to authorised
people. Those people make every decision. AIPS does not:

- send commands to flight controls, engines or any other aircraft system;
- declare an aircraft airworthy, or authorise or prohibit a departure;
- unlock or influence the cockpit door;
- trigger an emergency landing function;
- present a correlation as a root cause.

The build enforces this in code. There is no interface to an aircraft, no approve or deny action
anywhere in the product, and the Guardian service refuses to run outside training mode.

## How we read the frameworks for Phase 0

| Framework | Position for Phase 0 | What we do now |
|---|---|---|
| DO-178C / DO-254 | Do not apply: nothing runs on board | Keep traceable requirements and tests so later airborne work is not a rewrite |
| ARP4754B / ARP4761A | Not mandatory for a ground tool; used as a template | Preliminary hazard analysis in `HAZARD_ANALYSIS.md` |
| DO-326A / DO-356A | Apply to onboard links, none exist yet | Threat model follows the same thinking (`THREAT_MODEL.md`) |
| DO-355 / ED-204 | Relevant: AIPS handles continuing-airworthiness data | Access control, audit trail, integrity checks, encryption in transit |
| EASA AI concept paper (Issue 02, proposed Issue 03) | Intended as Level 1 AI (assistance to humans) | Model cards with data lineage and evaluation, drift monitoring, rollback, human grading of all findings |
| EU AI Act | Classification unclear; needs legal review | Keep the advisory boundary and learning-assurance evidence from the start |
| GDPR and labour law | Phase 0 processes no personal or physiological data | Guardian sensing needs a DPIA and crew-representative agreement before any pilot |
| Operator SMS, Part-CAMO, Part-145 | AIPS output becomes input to the operator's processes | Findings are labelled with evidence grade and data age, and are never presented as decisions |

## What would change this position

Any of the following moves AIPS out of the advisory category and needs a new assessment:

- an interface that writes to, or forwards events into, an aircraft system (Phase 8);
- Guardian processing real crew sensor or camera data (Phase 6);
- a customer using AIPS output as the sole basis for a maintenance release or dispatch decision.

## Open items

1. Confirm the advisory classification with aviation counsel (PRD strategy item 1).
2. Get legal advice on EU AI Act classification.
3. Re-check framework versions and dates at each phase gate, since several are still moving.
