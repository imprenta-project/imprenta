import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App.js';
import './style.css';

const root = document.getElementById('root');
if (!root) {
  throw new Error('the preview page has no root to mount on');
}
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
