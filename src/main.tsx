/**
 * 主窗口入口
 */

import React from 'react';
import ReactDOM from 'react-dom/client';
import { MainWindow } from './components/MainWindow';
import './styles/tokens.css';
import { installStickyBridge } from './lib/bridge';

installStickyBridge();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <MainWindow />
  </React.StrictMode>
);
