import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Dialog } from '../../ui';
import { safeDiagnosticCode } from '../../error-code';
import type { FeedRemovalPreview, ModelFeedStatus } from './types';
import './model-feed.css';

export function ModelFeedPanel({ language, mutable = true, onChanged }: { language: 'zh' | 'en'; mutable?: boolean; onChanged?: () => void }) {
  const zh = language === 'zh';
  const [status, setStatus] = useState<ModelFeedStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [preview, setPreview] = useState<FeedRemovalPreview | null>(null);
  const [disable, setDisable] = useState(true);
  const active = useRef(false);
  const live = useRef(true);
  const load = useCallback(async () => {
    try { const result = await invoke<ModelFeedStatus>('model_feed_status'); if (live.current) { setStatus(result); setError(''); } }
    catch { if (live.current) setError(zh ? '暂时无法读取模型信息更新状态。' : 'Model feed status is unavailable.'); }
  }, [zh]);
  useEffect(() => { live.current = true; void load(); return () => { live.current = false; }; }, [load]);
  async function change(command: string, input?: unknown) {
    if (active.current || !mutable) return;
    active.current = true; setBusy(true); setError('');
    try {
      const result = await invoke<ModelFeedStatus>(command, input === undefined ? undefined : { input });
      if (live.current) { setStatus(result); setPreview(null); window.dispatchEvent(new Event('model-feed-changed')); onChanged?.(); }
    } catch (failure) {
      if (live.current) {
        setError(safeDiagnosticCode(failure, 'MODEL_FEED_UNAVAILABLE').toLowerCase().includes('stale') ? (zh ? '影响范围已变化，请重新查看后再移除。' : 'The affected sources changed. Review the impact again.') : (zh ? '操作未完成，请重试。当前已验证的信息会保留。' : 'The action did not complete. Retry; verified information is retained.'));
        setPreview(null);
      }
    } finally { active.current = false; if (live.current) setBusy(false); }
  }
  async function removePreview() {
    if (active.current || !mutable) return;
    active.current = true; setBusy(true); setError('');
    try { const result = await invoke<FeedRemovalPreview>('model_feed_remove_preview'); if (live.current) { setDisable(true); setPreview(result); } }
    catch { if (live.current) setError(zh ? '无法确认影响范围，请重新读取后再试。' : 'The removal impact could not be verified. Reload and retry.'); }
    finally { active.current = false; if (live.current) setBusy(false); }
  }
  const refusal = status?.verification.kind === 'discarded' ? status.verification.reason : null;
  const refusalText = refusal === 'feed_state_invalid' ? (zh ? '保存的更新状态已损坏。' : 'Saved feed state is damaged.')
    : refusal === 'feed_state_newer_schema' ? (zh ? '保存的状态来自更新版本，请使用相应版本打开。' : 'Saved state uses a newer format. Open it with a compatible client.')
    : refusal === 'feed_activation_requires_restart' ? (zh ? '更新未能完成激活。当前目录仍可使用，请重启后恢复更新。' : 'Activation did not finish. The previous catalog remains usable; restart to resume updates.')
    : refusal ? (zh ? '无法安全修改保存的更新状态。请保留现有文件并检查本地存储。' : 'Saved feed state cannot be safely changed. Preserve the files and check local storage.') : '';
  const date = (value: number | string | undefined) => value === undefined ? '—' : new Date(value).toLocaleString(language === 'zh' ? 'zh-CN' : 'en-US');
  const resultLabel: Record<string, string> = zh ? { ok: '已更新', not_modified: '没有新发布', already_current: '已是最新', not_published: '暂无发布', network: '获取失败', size: '内容超出限制', signature: '签名验证失败', schema: '格式不支持', validation: '内容校验失败', older_sequence: '已拒绝旧发布' } : { ok: 'Updated', not_modified: 'No new publication', already_current: 'Already current', not_published: 'No publication yet', network: 'Retrieval failed', size: 'Content exceeds limit', signature: 'Signature verification failed', schema: 'Unsupported format', validation: 'Content validation failed', older_sequence: 'Older publication rejected' };
  return <section className="model-feed-panel" aria-label={zh ? '模型信息更新' : 'Model feed'}>
    <div className="model-feed-heading"><div><h3>{zh ? '模型信息更新' : 'Model feed'}</h3><p>{zh ? '从 hiroute.ai 获取新模型信息，沿用你已配置的接入方式。' : 'New model information from hiroute.ai, using your configured connections.'}</p></div><span>{status ? status.retrieval_enabled ? (zh ? '自动获取已开启' : 'Automatic retrieval on') : (zh ? '自动获取已关闭' : 'Automatic retrieval off') : (zh ? '正在读取' : 'Loading')}</span></div>
    {status && <>
      {!status.disclosure_acknowledged && <div className="model-feed-disclosure"><p>{zh ? '模型信息由 hiroute.ai 提供，自动获取时不发送账户或使用标识。你可以在这里关闭自动获取。' : status.disclosure}</p><button className="btn btn-quiet" disabled={busy || !mutable} onClick={() => void change('model_feed_settings_apply', { acknowledge_disclosure: true })}>{zh ? '知道了' : 'Got it'}</button></div>}
      <dl className="model-feed-facts"><div><dt>{zh ? '当前副本' : 'Held publication'}</dt><dd>{status.held ? `${zh ? '序号' : 'Sequence'} ${status.held.sequence}` : (zh ? '未保存副本' : 'No held copy')}</dd></div><div><dt>{zh ? '验证状态' : 'Verification'}</dt><dd>{status.verification.kind === 'verified' ? (zh ? '已验证签名' : 'Signature verified') : status.verification.kind === 'discarded' ? (zh ? '副本未被采用' : 'Copy discarded') : (zh ? '暂无副本' : 'No copy')}</dd></div><div><dt>{zh ? '发布于' : 'Issued'}</dt><dd>{date(status.held?.issued_at)}</dd></div><div><dt>{zh ? '上次检查' : 'Last checked'}</dt><dd>{date(status.last_attempt?.at)}{status.last_attempt && <small>{resultLabel[status.last_attempt.result] ?? (zh ? '检查未完成' : 'Check incomplete')}</small>}</dd></div></dl>
      {status.held && <p className="field-help">{zh ? `已采用 ${status.counts.applied} 条 · 沿用内置 ${status.counts.ignored_embedded} 条 · 需新版本 ${status.counts.newer_client} 条 · 未能匹配 ${status.counts.unresolved} 条` : `${status.counts.applied} applied · ${status.counts.ignored_embedded} already embedded · ${status.counts.newer_client} need a newer client · ${status.counts.unresolved} unresolved`}</p>}
      {refusalText && <p className="callout warn" role="status">{refusalText}{status.recovery_available ? (zh ? '可确认重置缓存，保留损坏文件和最高已接受序号。' : 'You can confirm a cache reset. The damaged file and highest accepted sequence are preserved.') : ''}</p>}
      <div className="model-feed-actions"><button className="btn" disabled={busy || !mutable || !!refusal} onClick={() => void change('model_feed_refresh')}>{busy ? (zh ? '处理中…' : 'Working…') : (zh ? '立即检查' : 'Check now')}</button><button className="btn btn-quiet" disabled={busy || !mutable || !!refusal} onClick={() => void change('model_feed_settings_apply', { retrieval_enabled: !status.retrieval_enabled })}>{status.retrieval_enabled ? (zh ? '关闭自动获取' : 'Turn off automatic retrieval') : (zh ? '开启自动获取' : 'Turn on automatic retrieval')}</button><button className="btn btn-quiet" disabled={busy || !mutable || (!status.held && !status.recovery_available)} onClick={() => void removePreview()}>{status.recovery_available ? (zh ? '重置损坏缓存…' : 'Reset damaged cache…') : (zh ? '移除副本…' : 'Remove copy…')}</button></div>
    </>}
    {error && <p role="alert" className="field-error">{error} <button className="btn btn-quiet" disabled={busy} onClick={() => void load()}>{zh ? '重新读取' : 'Reload'}</button></p>}
    <Dialog open={Boolean(preview)} title={status?.recovery_available ? (zh ? '重置损坏缓存' : 'Reset damaged cache') : (zh ? '移除模型信息副本' : 'Remove model feed copy')} closeLabel={zh ? '取消' : 'Cancel'} closeDisabled={busy} onClose={() => setPreview(null)} footer={<><button className="btn" disabled={busy} onClick={() => setPreview(null)}>{zh ? '取消' : 'Cancel'}</button><button className="btn btn-primary" disabled={busy || !preview} onClick={() => preview && void change('model_feed_remove_apply', { accepted_digest: preview.digest, also_disable_retrieval: disable })}>{status?.recovery_available ? (zh ? '确认重置' : 'Confirm reset') : (zh ? '移除副本' : 'Remove copy')}</button></>}>
      {status?.recovery_available && <p>{zh ? '将保留损坏的状态文件并新建缓存状态。最高已接受序号不会降低。' : 'The damaged state file will be preserved and cache state recreated. The highest accepted sequence will not decrease.'}</p>}
      <p>{zh ? '这些来源中的新增模型将不能再用于检查、保存或发布新的路由。已发布路由及其会话和 Worker 继续运行。' : 'Feed models in these sources will no longer be available for checks, saves or publishing new routing plans. Published plans, sessions and Workers keep running.'}</p>
      {preview && <>{preview.sources.length ? <ul>{preview.sources.map(source => <li key={source.source_id}><strong>{source.display_name}</strong><span> · {source.model_configuration_ids.length} {zh ? '个模型' : 'models'}</span></li>)}</ul> : <p>{zh ? '没有保存的来源依赖此副本。' : 'No saved sources depend on this copy.'}</p>}<p>{zh ? `已发布的 ${preview.published_revisions.length} 个路由版本保持可用。最高已接受序号 ${preview.retained_sequence ?? '—'} 会保留，防止回滚。` : `${preview.published_revisions.length} published plan versions remain usable. Highest accepted sequence ${preview.retained_sequence ?? '—'} is retained to prevent rollback.`}</p></>}
      <label className="model-feed-checkbox"><input type="checkbox" checked={disable} disabled={busy} onChange={event => setDisable(event.target.checked)} /><span>{zh ? '同时关闭自动获取' : 'Also turn off automatic retrieval'}</span></label>
    </Dialog>
  </section>;
}
