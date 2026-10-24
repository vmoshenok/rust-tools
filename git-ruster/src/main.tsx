import { render } from "preact";
import "@xterm/xterm/css/xterm.css";
import "./styles.css";
import { App } from "./components/App";

render(<App />, document.getElementById("app")!);
