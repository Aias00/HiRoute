import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Plan } from '../../plan-editor';
import type { ModelAvailabilityResult } from '../model-feed/types';
import './trial.css';

export function NewModels({ language, active, plans, refreshVersion, onTry }: {
  language: 'zh' | 'en'; active: boolean; plans: Plan[]; refreshVersion: number; onTry(modelId?: string): void;
}) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  const [data, setData] = useState<ModelAvailabilityResult | null>(null), [error, setError] = useState(false), [retry, setRetry] = useState(0);
  useEffect(() => {
    if (!active) return;
    let current = true;
    void invoke<ModelAvailabilityResult>('model_availability').then(result => { if (current) { setData(result); setError(false); } }).catch(() => { if (current) setError(true); });
    return () => { current = false; };
  }, [active, refreshVersion, retry]);
  const used = new Set(plans.flatMap(plan => {
    const s = plan.desired.strategy;
    return [...s.candidates ?? [], ...s.economy ?? [], ...s.primary ?? [], ...s.routing?.branches.flatMap(b => [...b.candidates, ...b.primary_candidates]) ?? []].map(c => c.binding_id);
  }));
  const entries = data?.featured.length ? data.featured.slice(0, 6).flatMap(entry => {
    const model = data.models.find(m => m.model_configuration_id === entry.model_configuration_id);
    return model ? [{ key: entry.id, model, headline: entry.headline[language], note: entry.note[language], date: entry.included_on }] : [];
  }) : data?.models.filter(m => m.origin.kind === 'embedded' && ['executable', 'needs_connection'].includes(m.availability.kind) && !m.binding_ids.some(id => used.has(id))).slice(0, 6).map(model => ({ key: model.model_configuration_id, model, headline: model.display_name, note: '', date: '' })) ?? [];
  return <section className="surface new-models"><header className="surface-header"><h2>{t('新模型', 'New models')}</h2><button className="btn btn-quiet" disabled={error || !data} onClick={() => onTry()}>{t('试用其他模型', 'Try another model')}</button></header>
    {error && <div className="callout warn" role="alert"><span>{t('模型目录暂时无法刷新，请重试后开始试用。', 'The model catalog could not refresh. Retry before starting a trial.')}</span><button className="btn" onClick={() => setRetry(n => n + 1)}>{t('重试', 'Retry')}</button></div>}
    <div className="new-models-list">{entries.map(({ key, model, headline, note, date }) => <article key={key} className="new-model-card"><h3>{headline}</h3>{note && <p>{note}</p>}<p className="field-help">{model.origin.kind === 'feed' ? t(`模型更新 · 序号 ${model.origin.sequence}`, `Model feed · sequence ${model.origin.sequence}`) : model.origin.kind === 'embedded' ? t(`内置目录 · ${model.origin.client_version}`, `Built-in catalog · ${model.origin.client_version}`) : t('已发布版本', 'Published revision')}{date && ` · ${date}`}</p>
      {['rolling-alias', 'dynamic-route', 'unknown'].includes(model.identity_stability ?? 'unknown') && <p className="field-help">{t('同名模型可能由供应商更新；公告日期不代表实际执行版本。', 'The provider may update this alias; an announcement date does not identify the executed version.')}</p>}
      <div className="actions"><button className="btn" disabled={error || model.availability.kind === 'needs_newer_client' || model.origin.kind === 'published'} onClick={() => onTry(model.model_configuration_id)}>{model.availability.kind === 'needs_newer_client' ? t(`需要 HiRoute ${model.availability.version}`, `Requires HiRoute ${model.availability.version}`) : model.binding_ids.length ? t('试用', 'Try it') : t('接入并试用', 'Connect and try')}</button></div>
    </article>)}</div>
    {!error && data && !entries.length && <p className="field-help">{t('暂无推荐；仍可从目录选择模型试用。', 'No featured models. You can still choose a model from the catalog.')}</p>}
  </section>;
}
