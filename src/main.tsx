import React from 'react';
import ReactDOM from 'react-dom/client';
import './styles/tokens.css';
import './styles/base.css';
import NodusApp from './NodusApp';
import FlyoutApp from './FlyoutApp';
import { windowKind } from './bridge';

// The tray flyout window loads the same bundle with ?w=quick → render the
// compact quick-controls instead of the full app (t13 Phase B2).
const isFlyout = windowKind() === 'quick';
const Root = isFlyout ? FlyoutApp : NodusApp;

// The flyout window is transparent (for rounded corners) → clear the page
// background so only the rounded .flyout shows; the corners stay see-through.
if (isFlyout) {
  document.documentElement.style.background = 'transparent';
  document.body.style.background = 'transparent';
  const boot = document.getElementById('boot');
  if (boot) boot.style.background = 'transparent';
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  React.createElement(React.StrictMode, null, React.createElement(Root)),
);

// Hide the boot splash once React mounts (full app shell OR the flyout panel).
const boot = document.getElementById('boot');
const iv = setInterval(() => {
  if (document.querySelector('.app-shell, .qp, .flyout')) {
    if (boot) {
      boot.classList.add('hide');
      setTimeout(() => boot.remove(), 450);
    }
    clearInterval(iv);
  }
}, 80);
setTimeout(() => clearInterval(iv), 8000);
