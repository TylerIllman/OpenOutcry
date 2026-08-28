import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";

import { Landing } from "./routes/Landing";
import { CreateSession } from "./routes/CreateSession";
import { HostSession } from "./routes/HostSession";
import { Join } from "./routes/Join";
import { PlayerSession } from "./routes/PlayerSession";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<Landing />} />
        <Route path="/host/new" element={<CreateSession />} />
        <Route path="/host/:code" element={<HostSession />} />
        <Route path="/join/:code" element={<Join />} />
        <Route path="/play/:code" element={<PlayerSession />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </BrowserRouter>
  </StrictMode>,
);
