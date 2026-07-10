import { useEffect, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { hasCredential, saveConnection, saveCredential, testConnection } from "@/lib/tauri";
import type { AuthType, ConnectionInput, ConnectionProfile, SecretKind } from "@/types/connection";

interface FormState {
  name: string;
  host: string;
  port: string;
  username: string;
  authType: AuthType;
  keyPath: string;
  tags: string;
  secret: string;
  color: string;
}

const EMPTY_FORM: FormState = {
  name: "",
  host: "",
  port: "22",
  username: "",
  authType: "password",
  keyPath: "",
  tags: "",
  secret: "",
  color: "",
};

// Dark, low-saturation tints close to the terminal's default background
// lightness so foreground text stays readable regardless of which is picked.
const PRESET_COLORS = [
  "#2a1216",
  "#2a1c0d",
  "#2a230a",
  "#0f2416",
  "#0a2422",
  "#0d1c2a",
  "#15132a",
  "#20122a",
  "#2a1220",
];

function profileToForm(profile: ConnectionProfile): FormState {
  return {
    name: profile.name,
    host: profile.host,
    port: String(profile.port),
    username: profile.username,
    authType: profile.auth_type,
    keyPath: profile.key_path ?? "",
    tags: profile.tags.join(", "),
    secret: "",
    color: profile.color ?? "",
  };
}

interface ConnectionEditorDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  connection: ConnectionProfile | null;
  onSaved: (profile: ConnectionProfile) => void;
}

export function ConnectionEditorDialog({
  open,
  onOpenChange,
  connection,
  onSaved,
}: ConnectionEditorDialogProps) {
  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [credentialSaved, setCredentialSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [testStatus, setTestStatus] = useState<"idle" | "testing" | "success" | "error">("idle");
  const [testMessage, setTestMessage] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setTestStatus("idle");
    setTestMessage(null);
    setForm(connection ? profileToForm(connection) : EMPTY_FORM);

    if (connection) {
      const kind: SecretKind = connection.auth_type === "password" ? "password" : "passphrase";
      hasCredential(connection.id, connection.username, kind)
        .then(setCredentialSaved)
        .catch(() => setCredentialSaved(false));
    } else {
      setCredentialSaved(false);
    }
  }, [open, connection]);

  async function handleBrowseKey() {
    const path = await openFileDialog({ multiple: false, title: "Select private key file" });
    if (typeof path === "string") {
      setForm((f) => ({ ...f, keyPath: path }));
    }
  }

  function toInput(): ConnectionInput {
    return {
      name: form.name.trim(),
      host: form.host.trim(),
      port: Number(form.port) || 22,
      username: form.username.trim(),
      auth_type: form.authType,
      key_path: form.authType === "key" ? form.keyPath.trim() || null : null,
      tags: form.tags
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean),
      color: form.color || null,
    };
  }

  async function persist(): Promise<ConnectionProfile> {
    const saved = await saveConnection(connection?.id ?? null, toInput());
    if (form.secret.trim()) {
      const kind: SecretKind = form.authType === "password" ? "password" : "passphrase";
      await saveCredential(saved.id, saved.username, kind, form.secret.trim());
    }
    return saved;
  }

  async function handleSave() {
    setSaving(true);
    setError(null);
    try {
      const saved = await persist();
      onSaved(saved);
      onOpenChange(false);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleTestConnect() {
    setTestStatus("testing");
    setTestMessage(null);
    setError(null);
    try {
      await testConnection(connection?.id ?? null, toInput(), form.secret.trim() || null);
      setTestStatus("success");
    } catch (err) {
      setTestStatus("error");
      setTestMessage(String(err));
    }
  }

  const secretLabel = form.authType === "password" ? "Password" : "Key passphrase";
  const secretPlaceholder = credentialSaved ? "•••••••• (saved — leave blank to keep)" : "";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{connection ? "Edit connection" : "New connection"}</DialogTitle>
        </DialogHeader>

        <div className="grid gap-3">
          <div className="grid gap-1.5">
            <Label htmlFor="name">Name</Label>
            <Input
              id="name"
              value={form.name}
              onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              placeholder="My server"
            />
          </div>

          <div className="grid grid-cols-3 gap-3">
            <div className="col-span-2 grid gap-1.5">
              <Label htmlFor="host">Host</Label>
              <Input
                id="host"
                value={form.host}
                onChange={(e) => setForm((f) => ({ ...f, host: e.target.value }))}
                placeholder="192.168.1.10"
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="port">Port</Label>
              <Input
                id="port"
                inputMode="numeric"
                value={form.port}
                onChange={(e) => setForm((f) => ({ ...f, port: e.target.value }))}
              />
            </div>
          </div>

          <div className="grid gap-1.5">
            <Label htmlFor="username">Username</Label>
            <Input
              id="username"
              value={form.username}
              onChange={(e) => setForm((f) => ({ ...f, username: e.target.value }))}
              placeholder="root"
            />
          </div>

          <div className="grid gap-1.5">
            <Label>Auth type</Label>
            <Select
              value={form.authType}
              onValueChange={(value) => setForm((f) => ({ ...f, authType: value as AuthType, secret: "" }))}
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="password">Password</SelectItem>
                <SelectItem value="key">Private key</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {form.authType === "key" && (
            <div className="grid gap-1.5">
              <Label htmlFor="keyPath">Private key file</Label>
              <div className="flex gap-2">
                <Input
                  id="keyPath"
                  value={form.keyPath}
                  onChange={(e) => setForm((f) => ({ ...f, keyPath: e.target.value }))}
                  placeholder="C:\Users\you\.ssh\id_ed25519"
                />
                <Button type="button" variant="outline" onClick={handleBrowseKey}>
                  Browse
                </Button>
              </div>
            </div>
          )}

          <div className="grid gap-1.5">
            <Label htmlFor="secret">{secretLabel}</Label>
            <Input
              id="secret"
              type="password"
              value={form.secret}
              onChange={(e) => setForm((f) => ({ ...f, secret: e.target.value }))}
              placeholder={secretPlaceholder}
            />
          </div>

          <div className="grid gap-1.5">
            <Label htmlFor="tags">Tags</Label>
            <Input
              id="tags"
              value={form.tags}
              onChange={(e) => setForm((f) => ({ ...f, tags: e.target.value }))}
              placeholder="prod, web (comma separated)"
            />
          </div>

          <div className="grid gap-1.5">
            <Label>Terminal color</Label>
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                title="Default"
                onClick={() => setForm((f) => ({ ...f, color: "" }))}
                className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-border ${
                  form.color === "" ? "ring-2 ring-ring ring-offset-2 ring-offset-background" : ""
                }`}
              >
                <span className="h-3 w-3 rounded-full bg-muted-foreground/40" />
              </button>
              {PRESET_COLORS.map((color) => (
                <button
                  key={color}
                  type="button"
                  title={color}
                  onClick={() => setForm((f) => ({ ...f, color }))}
                  style={{ backgroundColor: color }}
                  className={`h-6 w-6 shrink-0 rounded-full border border-border ${
                    form.color === color ? "ring-2 ring-ring ring-offset-2 ring-offset-background" : ""
                  }`}
                />
              ))}
              <input
                type="color"
                value={form.color || "#0b0f19"}
                onChange={(e) => setForm((f) => ({ ...f, color: e.target.value }))}
                title="Custom color"
                className="h-6 w-6 shrink-0 cursor-pointer rounded-full border border-border p-0"
              />
            </div>
          </div>

          {error && <p className="text-sm text-destructive">{error}</p>}
          {testStatus === "success" && <p className="text-sm text-emerald-500">Connected successfully.</p>}
          {testStatus === "error" && <p className="text-sm text-destructive">{testMessage}</p>}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={handleTestConnect}
            disabled={testStatus === "testing" || saving}
          >
            {testStatus === "testing" ? "Testing…" : "Test connect"}
          </Button>
          <Button type="button" onClick={handleSave} disabled={saving}>
            {saving ? "Saving…" : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
