import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// 禁用 WebView2/Edge 默认右键菜单（粘贴等请用 Ctrl+V）
window.addEventListener("contextmenu", (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
