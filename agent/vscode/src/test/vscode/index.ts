// Runs inside a real VS Code (`--extensionTestsPath`, see README): opens,
// edits and saves a file in the workspace so the extension emits events.
// The calling script checks what reached the agent's buffer.
import * as assert from 'node:assert/strict';
import * as path from 'node:path';
import * as vscode from 'vscode';

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function run(): Promise<void> {
  const ext = vscode.extensions.getExtension('codesecretary.codesecretary');
  assert.ok(ext, 'extension is installed');
  await ext.activate();
  const folder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(folder, 'a workspace folder is open');

  const uri = vscode.Uri.file(path.join(folder.uri.fsPath, 'src', 'main.rs'));
  const doc = await vscode.workspace.openTextDocument(uri);
  const editor = await vscode.window.showTextDocument(doc);
  for (let i = 0; i < 5; i++) {
    await editor.edit((b) => b.insert(new vscode.Position(0, 0), `// edit ${i}\n`));
    await wait(200);
  }
  assert.ok(await doc.save(), 'saved');
  // Past the 30 s edit window plus a flush, so nothing depends on shutdown timing.
  await wait(37_000);
}
