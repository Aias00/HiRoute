import { useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ModelConnectionForm } from '../../../src/features/model-connections/ModelConnectionForm';
import { newModelConnectionDraft } from '../../../src/product/ModelManagementPage';
import { Models } from '../../../src/features/models/Models';
import type { ModelConnectionBackend } from '../../../src/features/model-connections/types';
import { mockProductInvoke, readyManagement } from './product-fixtures';
import { PresentationRoot } from '../../../src/ui/PresentationRoot';
import '../../../src/occami/styles.css';

const params = new URLSearchParams(location.search);
const kind = params.get('kind') === 'free' ? 'free' : params.get('kind') === 'custom' ? 'custom' : 'known';
const trace = { checks: [] as unknown[], registered: [] as string[], released: [] as string[], saves: [] as unknown[] };
const pendingChecks: (() => void)[] = [];
Object.assign(window, { connectionFlowTrace: trace, finishConnectionFlowCheck: () => pendingChecks.shift()?.() });

function Harness() {
  const [open, setOpen] = useState(true);
  const [savedName, setSavedName] = useState('');
  const appendTo = params.has('append') ? { ...readyManagement.sources[0], display_name: '已有 · 接入', state: 'disabled' as const } : undefined;
  const initial = useMemo(() => appendTo ? { ...newModelConnectionDraft('custom'),
    existing_source_id: appendTo.source_id, display_name: appendTo.display_name, base_url: 'https://api.example.test',
  } : newModelConnectionDraft(kind), []);
  const language = params.get('lang') === 'en' ? 'en' : 'zh';
  const backend = useMemo<ModelConnectionBackend>(() => {
    const invoke = async (command: string, payload?: Record<string, unknown>) => mockProductInvoke(command, payload, kind === 'custom' && !appendTo ? 'unknown_model' : 'ready') as any;
    const check = async (command: string, request: any) => {
      if (params.get('hold_check')) await new Promise<void>(resolve => pendingChecks.push(resolve));
      const result = await invoke(command, { request });
      const ref = request.input_candidate?.candidate_ref ?? 'candidate/no-auth';
      return { ...result, candidate: { ...result.candidate, candidate: { candidate_ref: ref, candidate_revision: 2 },
        correlation: { ...result.candidate.correlation, candidate_ref: ref } } };
    };
    return {
      listConnectionOptions: () => invoke('compute_connection_options'),
      registerProtectedInput: async secret => {
        // Test labels identify fixture credentials without putting them into a wire draft.
        trace.registered.push(secret === 'fixture-key-a' ? 'a' : secret === 'fixture-key-b' ? 'b' : 'other');
        return { input_candidate: { candidate_ref: `input/${trace.registered.length}`, candidate_revision: 1 } };
      },
      releaseProtectedInput: async input => { trace.released.push(input.candidate_ref); },
      checkModelConnection: async request => { trace.checks.push(request); setSavedName(request.draft.display_name); return check('check_model_connection', request); },
      checkRegisteredModelConnection: async request => { trace.checks.push(request); setSavedName(request.display_name); return check('check_registered_model_connection', request); },
      cancelModelConnectionCheck: async () => undefined,
      previewComputeSave: async change => { trace.saves.push(change); return invoke('preview_compute_save', { change }); },
      applyComputeSave: request => invoke('apply_compute_save', { request }),
      getComputeSaveResult: operation => invoke('get_compute_save_result', { operation }),
    };
  }, []);
  const management = useMemo(() => ({ ...readyManagement, sources: readyManagement.sources.map(source =>
    source.provenance === 'registered' ? { ...source, display_name: savedName || '团队模型服务' } : source) }), [savedName]);
  return <PresentationRoot language={language} theme={params.get('theme') === 'dark' ? 'dark' : 'light'} textScale={params.get('scale') === '2' ? 2 : 1}>
    <Models language={language} snapshot={management} mutable onRefresh={async () => undefined}
      onPrepareKeyInput={async () => ({ input_candidate: { candidate_ref: 'input/saved', candidate_revision: 1 }, key_id: 'credential/test-saved' })}
      onPreview={async () => { throw new Error('Credential editing is outside this fixture'); }}
      onApply={async () => 'saved'} onDiscardProtectedInputs={() => undefined} />
    {open && <ModelConnectionForm language={language} initialDraft={initial} expectedRevisions={readyManagement.revisions} mutable backend={backend}
      appendTo={appendTo} existingNames={params.has('occupied') ? ['百炼 · Token Plan', '百炼 · Token Plan · 2'] : []}
      onBack={() => setOpen(false)} onCancel={() => setOpen(false)} onSaveAccepted={() => undefined}
      onSaveResult={() => setOpen(false)} onSaveUncertain={() => setOpen(false)} restoreFocus={() => undefined} />}
  </PresentationRoot>;
}
createRoot(document.getElementById('root')!).render(<Harness />);
