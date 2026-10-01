import { useState } from "react";
import { RefreshCw } from "lucide-react";
import { useApp } from "../lib/store";
import { Button, Tabs } from "../components/ui";
import { ScreenHeader } from "../components/Shell";
import AgentsTab from "./AgentsTab";
import LocalTab from "./LocalTab";
import CloudTab from "./CloudTab";
import PoolTab from "./PoolTab";

type Tab = "agents" | "local" | "cloud" | "pool";

export default function AgentsModels() {
  const [tab, setTab] = useState<Tab>(() => (sessionStorage.getItem("agentsTab") as Tab) || "agents");
  const refreshAgents = useApp((s) => s.refreshAgents);
  const settings = useApp((s) => s.settings)!;
  const [refreshing, setRefreshing] = useState(false);
  const change = (t: Tab) => {
    setTab(t);
    try {
      sessionStorage.setItem("agentsTab", t);
    } catch {
      /* ignore */
    }
  };
  return (
    <div className="flex flex-col h-full">
      <ScreenHeader title="Agents & Models" subtitle="Paid agents plan and take hard steps; open-source agents run routine steps on local or cheap cloud models.">
        <Button
          className="no-drag"
          variant="ghost"
          loading={refreshing}
          onClick={async () => {
            setRefreshing(true);
            await refreshAgents(true, true).finally(() => setRefreshing(false));
          }}
        >
          <RefreshCw className="h-3.5 w-3.5" /> Re-detect
        </Button>
      </ScreenHeader>
      <div className="px-6 shrink-0">
        <Tabs
          value={tab}
          onChange={change}
          tabs={[
            { value: "agents", label: "Coding agents" },
            { value: "local", label: "Local models" },
            { value: "cloud", label: "Cloud providers" },
            { value: "pool", label: "Executor pool", count: settings.executor_pool.length },
          ]}
        />
      </div>
      <div className="flex-1 overflow-auto">
        <div className="max-w-[980px] mx-auto px-6 py-5">
          {tab === "agents" && <AgentsTab />}
          {tab === "local" && <LocalTab />}
          {tab === "cloud" && <CloudTab />}
          {tab === "pool" && <PoolTab />}
        </div>
      </div>
    </div>
  );
}
