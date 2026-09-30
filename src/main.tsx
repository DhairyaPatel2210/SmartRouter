import React from "react";
import ReactDOM from "react-dom/client";
import { brand } from "./brand";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <h1>{brand.productName}</h1>
  </React.StrictMode>,
);
