import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, cn, Empty } from "@cloudflare/kumo";
import { FolderSimplePlusIcon, RocketLaunchIcon } from "@phosphor-icons/react";
import { api, Project } from "./api";
import { Confirm, ConfirmRequest } from "./Confirm";
import { ErrorBoundary, ErrorNotice } from "./errors";
import { ProjectView } from "./ProjectView";
import { UpdateBanner } from "./UpdateBanner";
import "./App.css";

function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null);

  useEffect(() => {
    api.projectList().then(
      (list) => {
        setProjects(list);
        setSelected((s) => s ?? list[0]?.id ?? null);
      },
      (e) => setError(String(e)),
    );
  }, []);

  async function addProject() {
    const path = await open({ directory: true, title: "Choose a Kamal project folder" });
    if (!path) return;
    setError(null);
    try {
      const project = await api.projectAdd(path);
      setProjects((ps) => [...ps, project].sort((a, b) => a.name.localeCompare(b.name)));
      setSelected(project.id);
    } catch (e) {
      setError(String(e));
    }
  }

  async function removeProject(id: number) {
    await api.projectRemove(id);
    const rest = projects.filter((p) => p.id !== id);
    setProjects(rest);
    setSelected(rest[0]?.id ?? null);
  }

  const project = projects.find((p) => p.id === selected);

  return (
    <main className="flex h-full">
      <nav className="flex w-52 shrink-0 flex-col gap-1 overflow-auto border-r border-kumo-hairline bg-kumo-base p-3">
        <div className="text-xs text-kumo-subtle px-2 pb-1 uppercase tracking-wide">
          Projects
        </div>
        {projects.map((p) => (
          <button
            key={p.id}
            title={p.path}
            onClick={() => setSelected(p.id)}
            className={cn(
              "truncate rounded-md px-2 py-1.5 text-left text-sm hover:bg-kumo-tint",
              p.id === selected && "bg-kumo-tint font-medium",
            )}
          >
            {p.name}
          </button>
        ))}
        <Button variant="ghost" size="sm" icon={<FolderSimplePlusIcon />} className="mt-1 justify-start" onClick={addProject}>
          Add project
        </Button>
        {error && <ErrorNotice error={error} />}
        <UpdateBanner />
      </nav>
      {project ? (
        <ErrorBoundary key={project.id}>
          <ProjectView project={project} confirm={setConfirm} onRemove={() => removeProject(project.id)} />
        </ErrorBoundary>
      ) : (
        <div className="m-auto">
          <Empty
            icon={<RocketLaunchIcon size={32} />}
            title="No projects yet"
            description="Add a folder that contains config/deploy.yml or config/deploy.<destination>.yml."
            contents={
              <Button variant="primary" icon={<FolderSimplePlusIcon />} onClick={addProject}>
                Add project
              </Button>
            }
          />
        </div>
      )}
      {confirm && <Confirm request={confirm} onClose={() => setConfirm(null)} />}
    </main>
  );
}

export default App;
