import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/geist";
import "../index.css";
import "../design-tokens.css";
import { Frame, Gallery } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

installMadeUpApi();
const framed = new URLSearchParams(window.location.search).has("frame");
createRoot(document.getElementById("root")!).render(<StrictMode>{framed ? <Frame /> : <Gallery />}</StrictMode>);
