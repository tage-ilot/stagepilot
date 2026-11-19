import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/geist";
import "../index.css";
import "../design-tokens.css";
import { Frame, Gallery } from "./Gallery";
import { installMadeUpApi } from "./madeUpApi";

installMadeUpApi();
const query = new URLSearchParams(window.location.search);
const only = query.get("only")?.split(",").filter(Boolean);
createRoot(document.getElementById("root")!).render(<StrictMode>{query.has("frame") ? <Frame only={only} /> : <Gallery />}</StrictMode>);
