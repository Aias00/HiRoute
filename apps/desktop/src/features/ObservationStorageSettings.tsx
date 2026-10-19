import { useEffect, useState } from 'react';
import { invoke } from '../host/index.ts';
import { safeDiagnosticCode } from '../error-code';
import type { Language } from '../ui/preferences';

type Capacity = { max_active_bytes: number | null; active_bytes: number | null; measured_at_ms: number | null; state: 'idle' | 'pending' | 'blocked'; reason: string };
export function ObservationStorageSettings({ active, language }: { active: boolean; language: Language }) {
  const text = (zh: string, en: string) => language === 'zh' ? zh : en;
  const [value, setValue] = useState<Capacity | null>(null);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    if (!active) return;
    let current = true;
    void invoke<Capacity>('observation_capacity_get').then(value => {
      if (!current) return;
      setValue(value); setDraft(value.max_active_bytes === null ? '' : String(value.max_active_bytes / 1_000_000_000));
    }).catch(error => { if (current) setError(safeDiagnosticCode(error, 'STORAGE_UNAVAILABLE')); });
    return () => { current = false; };
  }, [active]);
  const limit = draft.trim() === '' ? null : Number(draft) * 1_000_000_000;
  const valid = limit === null || (Number.isSafeInteger(limit) && limit > 0);
  async function save() {
    if (busy || !valid) return;
    setBusy(true); setError('');
    try {
      const saved = await invoke<Capacity>('observation_capacity_set', { input: { max_active_bytes: limit } });
      setValue(saved); setDraft(saved.max_active_bytes === null ? '' : String(saved.max_active_bytes / 1_000_000_000));
    } catch (error) { setError(safeDiagnosticCode(error, 'STORAGE_UNAVAILABLE')); }
    finally { setBusy(false); }
  }
  return <>
    <div className="settings-row">
      <div className="settings-copy"><strong>{text('记录容量上限', 'Record storage limit')}</strong>
        <span>{text('默认上限 20GB，记录通常保留 7 天。容量超限会分批清理最旧的已完成请求，可能不足 7 天。留空并保存可关闭容量回收。', 'Default: 20 GB. Records normally last 7 days; exceeding the limit gradually clears the oldest completed requests sooner. Clear the field and save to disable capacity retention.')}</span>
        <span>{text('按间隔回收，空间释放可能延后；封存的旧历史与恢复副本不计入此上限。', 'Reclamation is spaced and space release may lag. Archived history and recovery copies are outside this limit.')}</span>
        {value?.active_bytes !== null && value?.active_bytes !== undefined && <span>{text('最近测量', 'Last measured')}: {(value.active_bytes / 1_000_000_000).toFixed(2)} GB · {value.state === 'blocked' ? text('等待空间释放或可回收记录', 'Waiting for space release or eligible records') : value.state === 'pending' ? text('分批处理中', 'Processing gradually') : text('正常', 'Ready')}</span>}
      </div>
      <div className="worker-settings-control">
        <input className="input worker-settings-input" type="number" min="0.000000001" step="any" inputMode="decimal" aria-label={text('记录容量上限（GB）', 'Record storage limit (GB)')} placeholder={value === null ? text('读取中…', 'Loading…') : text('已关闭', 'Disabled')} value={draft} disabled={busy || value === null} onChange={event => setDraft(event.target.value)} />
        <span>GB</span><button className="btn" type="button" disabled={busy || value === null || !valid || limit === value.max_active_bytes} onClick={() => void save()}>{busy ? text('保存中…', 'Saving…') : text('保存', 'Save')}</button>
      </div>
    </div>
    {(!valid || error) && <div className="settings-inline-error" role="status">{!valid ? text('请输入正数，或留空关闭。', 'Enter a positive value, or leave empty to disable.') : text(`容量设置暂时不可用（${error}）`, `Storage settings unavailable (${error})`)}</div>}
  </>;
}
