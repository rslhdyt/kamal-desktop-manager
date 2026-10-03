import { useState } from "react";
import { Badge, Button, Dialog, Input } from "@cloudflare/kumo";

export type ConfirmRequest = {
  title: string;
  description?: string;
  /** User must type this exactly to confirm (production). */
  typed?: string;
  /** Ask for a free-text value (e.g. lock message). */
  input?: string;
  destructive?: boolean;
  onConfirm: (value: string) => void;
};

export function Confirm({ request, onClose }: { request: ConfirmRequest; onClose: () => void }) {
  const [value, setValue] = useState("");
  const ok = request.typed ? value === request.typed : request.input ? value.trim() !== "" : true;

  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog size="lg" className="p-6">
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (!ok) return;
            onClose();
            request.onConfirm(value);
          }}
        >
          <Dialog.Title className="text-lg font-semibold">{request.title}</Dialog.Title>
          {request.typed && (
            <div>
              <Badge variant="destructive">production</Badge>
            </div>
          )}
          {request.description && <Dialog.Description className="text-kumo-subtle">{request.description}</Dialog.Description>}
          {request.typed && (
            <Input
              autoFocus
              label={
                <>
                  Type <code className="font-mono">{request.typed}</code> to confirm
                </>
              }
              value={value}
              onChange={(e) => setValue(e.target.value)}
            />
          )}
          {request.input && (
            <Input autoFocus aria-label={request.input} placeholder={request.input} value={value} onChange={(e) => setValue(e.target.value)} />
          )}
          <div className="flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" variant={request.destructive ? "destructive" : "primary"} disabled={!ok}>
              Confirm
            </Button>
          </div>
        </form>
      </Dialog>
    </Dialog.Root>
  );
}
