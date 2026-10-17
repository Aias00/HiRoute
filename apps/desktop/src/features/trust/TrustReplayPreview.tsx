import { useEffect, useState } from 'react';
import { observationRead } from '../observation-client';
import type { TrustReplay } from './types';

export type ReplayScope = { planId: string; revision: number; branchId: string; active: boolean };
export function TrustReplayPreview({ scope, threshold, language }: { scope: ReplayScope; threshold: number; language: 'zh' | 'en' }) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  const [result, setResult] = useState<{ key: string; data: TrustReplay } | null>(null);
  const [failed, setFailed] = useState('');
  const [retry, setRetry] = useState(0);
  const [visible, setVisible] = useState(document.visibilityState === 'visible');
  const key = `${scope.planId}/${scope.revision}/${scope.branchId}/${threshold}`;
  useEffect(() => { const update = () => setVisible(document.visibilityState === 'visible'); document.addEventListener('visibilitychange', update); return () => document.removeEventListener('visibilitychange', update); }, []);
  useEffect(() => {
    if (!scope.active || !visible || !Number.isInteger(threshold) || threshold < 0 || threshold > 1000) return;
    let current = true;
    const abort = new AbortController();
    setFailed('');
    const timer = window.setTimeout(() => {
      observationRead<TrustReplay>('plan_trust_replay', { plan_id: scope.planId, revision: scope.revision, branch_id: scope.branchId, thresholds_millis: [threshold] }, abort.signal)
        .then(data => { if (current) setResult({ key, data }); })
        .catch(() => { if (current) setFailed(key); });
    }, 250);
    return () => { current = false; abort.abort(); window.clearTimeout(timer); };
  }, [key, scope.planId, scope.revision, scope.branchId, scope.active, threshold, retry, visible]);
  const data = result?.key === key ? result.data : null;
  const row = data?.thresholds.find(r => r.threshold_millis === threshold);
  const share = (n: number | null | undefined) => n == null ? '—' : `${Math.round(n * 100)}%`;
  const reasons: Record<string, string> = { heuristic: t('规则判断', 'Heuristic'), decision_fallback: t('决策回退', 'Decision fallback'), single_group: t('单组', 'Single group'), availability_relay: t('可用性接力', 'Availability relay'), degree_unavailable: t('概率未知', 'Probability unavailable') };
  return <div className="trust-replay" aria-live="polite"><strong>{t('历史回放估算', 'Historical replay estimate')}</strong><p className="field-help">{t(`发布修订版 ${scope.revision} · 最近 7 天的保留记录`, `Published revision ${scope.revision} · Retained records from the last 7 days`)}</p>
    {failed === key ? <p role="alert">{t('暂时无法读取回放。', 'Replay could not be read.')} <button type="button" className="btn" onClick={() => setRetry(n => n + 1)}>{t('重试', 'Retry')}</button></p> : !data || !row ? <p>{t('正在读取回放…', 'Reading replay…')}</p> : <>
      {data.included === 0 ? <p>{t('没有可用于回放的记录，不估算占比。', 'No usable recorded evidence; shares are not estimated.')}</p> : <p>{t(`阈值 ${threshold / 1000}：常规 ${row.regular}（${share(row.regular_share)}），主力 ${row.primary}（${share(row.primary_share)}），其中 ${row.protected_as_primary} 个仍由主力保护。原记录常规占比 ${share(data.recorded_share_at_current)}。`, `At ${threshold / 1000}: Regular ${row.regular} (${share(row.regular_share)}), Primary ${row.primary} (${share(row.primary_share)}), including ${row.protected_as_primary} protected stages. Recorded Regular share: ${share(data.recorded_share_at_current)}.`)}</p>}
      {Object.entries(data.excluded_by_reason).length > 0 && <p className="field-help">{t('未纳入', 'Excluded')}: {Object.entries(data.excluded_by_reason).map(([reason, count]) => `${reasons[reason] ?? reason} ${count}`).join(' · ')}</p>}
      <p className="field-help">{t(`这是历史分配估算，不是预测。${row.newly_regular} 个阶段将首次交给常规组，原来由主力处理，没有常规胜任证据。`, `This estimates historical allocation, not future results. ${row.newly_regular} stages would newly go to Regular; Primary handled them, so they provide no Regular competence evidence.`)}</p>
    </>}
  </div>;
}
