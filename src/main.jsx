import React from 'react';
import ReactDOM from 'react-dom/client';
import './styles.css';
import App from './app.jsx';

const root = ReactDOM.createRoot(document.getElementById('root'));
root.render(React.createElement(App));

// Hide boot splash once React mounts
const boot = document.getElementById('boot');
const iv = setInterval(() => {
  if (document.querySelector('.app')) {
    if (boot) { boot.classList.add('hide'); setTimeout(() => boot.remove(), 450); }
    clearInterval(iv);
  }
}, 80);
setTimeout(() => clearInterval(iv), 8000);
