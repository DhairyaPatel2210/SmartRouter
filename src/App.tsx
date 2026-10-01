import { lazy, Suspense, useEffect } from "react";
import { useApp } from "./lib/store";
import { Approvals, DropZone, Notices, Sidebar, useShortcuts } from "./components/Shell";
import { StatusBar } from "./statusbar/StatusBar";
import Home from "./home/Home";
import { Spinner } from "./components/ui";
import { Celebration } from "./components/Celebration";

// Heavy screens load only when opened (CodeMirror, React Flow, charts).
const RunView = lazy(() => import("./runs/RunView"));
const Library = lazy(() => import("./library/Library"));
const AgentsModels = lazy(() => import("./agents/AgentsModels"));
const Telemetry = lazy(() => import("./telemetry/Telemetry"));
const Workflows = lazy(() => import("./workflow/Workflows"));
const Settings = lazy(() => import("./settings/Settings"));
const Onboarding = lazy(() => import("./onboarding/Onboarding"));

export default function App() {
  const ready = useApp((s) => s.ready);
  const bootError = useApp((s) => s.bootError);
  const screen = useApp((s) => s.screen);
  const onboarding = useApp((s) => s.onboardingOpen);
  const theme = useApp((s) => s.settings?.theme ?? "system");
  useShortcuts();

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      // Switch themes without animating every color transition.
      const root = document.documentElement;
      root.classList.add("theme-switching");
      root.classList.toggle("dark", theme === "dark" || (theme === "system" && mq.matches));
      requestAnimationFrame(() => requestAnimationFrame(() => root.classList.remove("theme-switching")));
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);

  if (bootError) {
    return (
      <div className="h-full flex items-center justify-center p-8 text-center">
        <div>
          <div className="font-semibold">The app couldn't start</div>
          <pre className="mt-2 text-xs text-muted whitespace-pre-wrap selectable">{bootError}</pre>
        </div>
      </div>
    );
  }
  if (!ready) {
    return (
      <div data-tauri-drag-region className="h-full flex items-center justify-center text-faint">
        <Spinner />
      </div>
    );
  }
  if (onboarding) {
    return (
      <Suspense fallback={<Loading />}>
        <Onboarding />
      </Suspense>
    );
  }
  return (
    <div className="h-full flex flex-col">
      <div className="flex-1 flex min-h-0">
        <Sidebar />
        <main className="flex-1 min-w-0 flex flex-col bg-bg">
          <Notices />
          <div className="flex-1 min-h-0">
            <Suspense fallback={<Loading />}>
              {screen === "home" && <Home />}
              {screen === "run" && <RunView />}
              {screen === "library" && <Library />}
              {screen === "agents" && <AgentsModels />}
              {screen === "telemetry" && <Telemetry />}
              {screen === "workflows" && <Workflows />}
              {screen === "settings" && <Settings />}
            </Suspense>
          </div>
        </main>
      </div>
      <StatusBar />
      <Approvals />
      <DropZone />
      <Celebration />
    </div>
  );
}

function Loading() {
  return (
    <div className="h-full flex items-center justify-center text-faint">
      <Spinner />
    </div>
  );
}
