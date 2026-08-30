import { render } from "@solidjs/web";
import App from "./App";
import "./app.css";

const mount = document.getElementById("app");

// Thrown rather than narrowed away, because a missing mount point means index.html and this file
// have drifted apart — a build mistake, not a runtime condition to handle
if (!mount) throw new Error("index.html is missing the #app element");

render(() => <App />, mount);
