import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App';
import { routeExternalLinksToBrowser } from './lib/externalLinks';
import './stoatworks-theme.css';
import './styles.css';

// The About dialog's data file carries a fallback version; this is the one
// the build produced. Spread, not assign: about-data.js may not have run yet.
window.STOATWORKS_ABOUT = { ...window.STOATWORKS_ABOUT, version: __APP_VERSION__ };

// Tauri's webview silently refuses target="_blank"; the About links need it.
routeExternalLinksToBrowser();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
