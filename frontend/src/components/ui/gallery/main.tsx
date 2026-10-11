import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/inter";
import "../../../index.css";
import "../../../design-tokens.css";
import { Gallery } from "./Gallery";

createRoot(document.getElementById("root")!).render(
  <StrictMode><Gallery /></StrictMode>,
);
