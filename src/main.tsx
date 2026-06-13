import React from 'react';
import ReactDOM from 'react-dom/client';
import './styles/tokens.css';
import './styles/base.css';
import NodusApp from './NodusApp';

ReactDOM.createRoot(document.getElementById('root')!).render(
  React.createElement(React.StrictMode, null, React.createElement(NodusApp)),
);

// Hide the boot splash once React mounts.
const boot = document.getElementById('boot');
const iv = setInterval(() => {
  if (document.querySelector('.app-shell')) {
    if (boot) {
      boot.classList.add('hide');
      setTimeout(() => boot.remove(), 450);
    }
    clearInterval(iv);
  }
}, 80);
setTimeout(() => clearInterval(iv), 8000);
