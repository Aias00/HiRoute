import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { ModelAvailabilityResult, ModelFeedStatus } from './types';

export function ModelOriginDetails({ modelId, language }: { modelId: string; language: 'zh' | 'en' }) {
  const [result, setResult] = useState<ModelAvailabilityResult | null>(null);
  const [status, setStatus] = useState<ModelFeedStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const zh = language === 'zh';
  useEffect(() => {
    let live = true;
    const refresh = () => { void Promise.all([invoke<ModelAvailabilityResult>('model_availability'), invoke<ModelFeedStatus>('model_feed_status')])
      .then(([models, feed]) => { if (live) { setResult(models); setStatus(feed); } }).catch(() => { if (live) { setResult(null); setStatus(null); } }); };
    refresh(); window.addEventListener('model-feed-changed', refresh);
    return () => { live = false; window.removeEventListener('model-feed-changed', refresh); };
  }, [modelId]);
  const model = result?.models.find(model => model.model_configuration_id === modelId);
  if (!model) return null;
  const updates = result?.featured.filter(entry => entry.model_configuration_id === modelId && entry.kind === 'model_update') ?? [];
  const hasFeedContent = model.origin.kind === 'feed' || updates.length > 0;
  async function acknowledge() {
    if (busy) return;
    setBusy(true); setError(false);
    try { setStatus(await invoke<ModelFeedStatus>('model_feed_settings_apply', { input: { acknowledge_disclosure: true } })); window.dispatchEvent(new Event('model-feed-changed')); }
    catch { setError(true); }
    finally { setBusy(false); }
  }
  return <section className="detail-section model-origin-details">
    <h3>{zh ? '模型信息来源' : 'Model information origin'}</h3>
    <p>{model.origin.kind === 'embedded' ? (zh ? `内置于 HiRoute ${model.origin.client_version}` : `Embedded in HiRoute ${model.origin.client_version}`) : model.origin.kind === 'feed' ? (zh ? `来自 hiroute.ai · 序号 ${model.origin.sequence} · ${model.origin.included_on} 收录` : `From hiroute.ai · sequence ${model.origin.sequence} · included ${model.origin.included_on}`) : (zh ? '已发布路由中的冻结信息' : 'Frozen information in published plans')}</p>
    {updates.map(update => <p key={update.id}>{zh ? `更新公告 ${update.included_on}` : `Update announced ${update.included_on}`} · {update.note[language]}</p>)}
    {model.identity_stability === 'rolling-alias' && <p className="field-help">{zh ? '供应商可能更新同一模型名背后的版本。历史评分可能混合多个版本；信息收录日、公告日和 feed 序号都不代表实际执行的模型版本。' : 'The provider may update the model behind this name. Historical scores may mix versions; inclusion dates, announcement dates and feed sequences do not identify the model version that executed.'}</p>}
    {model.availability.kind === 'used_by_published_plans' && <p className="field-help">{zh ? `仅保留在 ${model.availability.count} 个已发布版本中。不能用于新发布。` : `Retained in ${model.availability.count} published versions. Unavailable for new publication.`}</p>}
    {hasFeedContent && status && !status.disclosure_acknowledged && <div className="model-feed-disclosure"><p>{zh ? '模型信息由 hiroute.ai 自动提供，不发送账户或使用标识。可在“模型信息更新”中关闭。' : 'Model information is retrieved automatically from hiroute.ai without account or usage identifiers. Turn it off in Model feed.'}</p><button className="btn btn-quiet" disabled={busy} onClick={() => void acknowledge()}>{zh ? '知道了' : 'Got it'}</button>{error && <p role="alert">{zh ? '未能保存，请重试。' : 'Could not save. Try again.'}</p>}</div>}
  </section>;
}
