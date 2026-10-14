// Explicit local persistence only. Raw UI observations may contain private data;
// do not put these files in the repository or send them to the Jev API.
import {mkdtemp, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';

export async function saveTrace(result, directory = tmpdir()) {
  const runDir = await mkdtemp(join(directory, 'hiroute-cu-'));
  const file = join(runDir, 'trace.json');
  await writeFile(file, JSON.stringify(result, null, 2), {flag: 'wx', mode: 0o600});
  return file;
}
