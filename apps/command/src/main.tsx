import '@fontsource/barlow/400.css';
import '@fontsource/barlow/500.css';
import '@fontsource/barlow/600.css';
import '@fontsource/barlow-semi-condensed/500.css';
import '@fontsource/barlow-semi-condensed/600.css';
import '@fontsource/barlow-semi-condensed/700.css';
import './styles.css';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { bootstrap } from './api';
import { App } from './App';

const root = createRoot(document.getElementById('root')!);
bootstrap()
  .then(() => root.render(<StrictMode><App /></StrictMode>))
  .catch((e: Error) => root.render(
    <div className="boot"><div className="state state-error" role="alert"><strong>AIPS Command could not start.</strong> {e.message}</div></div>,
  ));
