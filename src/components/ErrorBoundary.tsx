import { Component, type ReactNode } from "react";
import { AlertTriangle } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "./ui";

/** Sends UI errors to the core log (terminal + app.log). Never throws. */
export function reportError(message: string, level: "error" | "warn" = "error") {
  invoke("ui_log", { level, message }).catch(() => {});
}

/** Keeps a crash in one screen from blanking the whole window. */
export class ErrorBoundary extends Component<{ children: ReactNode; resetKey?: string; onHome?: () => void }, { error: Error | null }> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: { componentStack?: string | null }) {
    reportError(`${error.message}\n${(info.componentStack ?? "").split("\n").slice(0, 6).join("\n")}`);
  }

  componentDidUpdate(prev: { resetKey?: string }) {
    if (prev.resetKey !== this.props.resetKey && this.state.error) this.setState({ error: null });
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="h-full flex items-center justify-center p-8">
        <div className="max-w-lg text-center">
          <div className="mx-auto h-12 w-12 rounded-2xl bg-bad-soft text-bad flex items-center justify-center">
            <AlertTriangle className="h-6 w-6" />
          </div>
          <div className="mt-3 font-semibold">This screen hit a problem</div>
          <p className="text-muted text-[12.5px] mt-1">Runs keep going in the background. The details are in the app log.</p>
          <pre className="mt-3 text-left text-[11.5px] bg-panel-2 rounded-lg p-3 whitespace-pre-wrap selectable max-h-40 overflow-auto">{this.state.error.message}</pre>
          <div className="mt-4 flex justify-center gap-2">
            <Button onClick={() => this.setState({ error: null })}>Try again</Button>
            {this.props.onHome && <Button variant="primary" onClick={() => { this.setState({ error: null }); this.props.onHome?.(); }}>Back to Home</Button>}
          </div>
        </div>
      </div>
    );
  }
}
