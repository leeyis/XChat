import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import CaptureEditor from "./CaptureEditor.jsx";
import { createXChatModule } from "./xchat.js";
import "./styles.css";

const workspace = createXChatModule();
const root = createRoot(document.getElementById("root"));
const requestedView = new URLSearchParams(globalThis.location?.search || "").get("view");
const render = (View, props = {}) => root.render(
  <StrictMode>
    <View workspace={workspace} {...props} />
  </StrictMode>,
);

// Screenshot windows can render without loading the chat UI and its dependencies.
if (requestedView === "capture-editor" || requestedView === "capture-pin") {
  render(CaptureEditor, { mode: requestedView === "capture-pin" ? "pin" : "editor" });
} else if (requestedView === "remote-toolbar") {
  import("./RemoteToolbar.jsx").then(({ default: Toolbar }) => render(Toolbar));
} else {
  import("./App.jsx").then(({ default: App }) => render(App));
}
