import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App.jsx";
import { createXChatModule } from "./xchat.js";
import "./styles.css";

const workspace = createXChatModule();

createRoot(document.getElementById("root")).render(
  <StrictMode>
    <App workspace={workspace} />
  </StrictMode>,
);
