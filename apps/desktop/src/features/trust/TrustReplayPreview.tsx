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
  const bar = (n: number | null | undefined) => Math.max(0, Math.min(100, (n ?? 0) * 100));
  const reasons: Record<string, string> = { heuristic: t('规则判断', 'Heuristic'), decision_fallback: t('决策回退', 'Decision fallback'), single_group: t('单组', 'Single group'), availability_relay: t('可用性接力', 'Availability relay'), degree_unavailable: t('概率未知', 'Probability unavailable') };
  const excluded = Object.entries(data?.excluded_by_reason ?? {}).filter(([, count]) => count > 0);
  const excludedTotal = excluded.reduce((sum, [, count]) => sum + count, 0);
  return <div className="trust-replay" aria-live="polite">
    <div className="trust-replay-head"><strong>{t('用你的记录回放这一档', 'Replay this level on your records')}</strong><span>{t(`当前发布的版本 · 最近 7 天${data ? ` · ${data.included + excludedTotal} 个阶段` : ''}`, `Active version · last 7 days${data ? ` · ${data.included + excludedTotal} stages` : ''}`)}</span></div>
    {failed === key ? <p className="trust-replay-state" role="alert">{t('暂时无法读取回放。', 'Replay could not be read.')} <button type="button" className="btn btn-quiet" onClick={() => setRetry(n => n + 1)}>{t('重试', 'Retry')}</button></p>
      : !data || !row ? <p className="trust-replay-state">{t('正在读取回放…', 'Reading replay…')}</p>
      : data.included === 0 ? <p className="trust-replay-state">{t('没有可用于回放的记录，不估算占比。', 'No usable recorded evidence; shares are not estimated.')}</p>
      : <>
        <div className="trust-replay-bar" role="img" aria-label={t(`阈值 ${threshold / 1000}：常规 ${share(row.regular_share)}，主力 ${share(row.primary_share)}`, `At ${threshold / 1000}: Regular ${share(row.regular_share)}, Primary ${share(row.primary_share)}`)}><svg viewBox="0 0 100 14" preserveAspectRatio="none" aria-hidden="true"><rect className="regular" width={bar(row.regular_share)} height="14" /><rect className="primary" x={bar(row.regular_share)} width={bar(row.primary_share)} height="14" /></svg></div>
        <div className="trust-replay-legend">
          <span><i className="regular" />{t('常规', 'Regular')} <strong>{share(row.regular_share)}</strong> <span>{t(`（${row.regular} 个）`, `(${row.regular})`)}</span></span>
          <span><i className="primary" />{t('主力', 'Primary')} <strong>{share(row.primary_share)}</strong> <span>{t(`（${row.primary} 个）`, `(${row.primary})`)}</span></span>
          <span className="muted">{t(`当前档位实际 ${share(data.recorded_share_at_current)} 交给常规`, `Recorded at the current level: ${share(data.recorded_share_at_current)} Regular`)}</span>
        </div>
      </>}
    {data && row && data.included > 0 && <p className="field-help">{[
      t(`阈值 ${threshold / 1000}：计入 ${data.included} 个阶段，其中 ${row.protected_as_primary} 个低分保护按主力计算`, `At ${threshold / 1000}: ${data.included} stages included; ${row.protected_as_primary} low-score protections count as Primary`),
      excluded.length ? t(`未计入 ${excludedTotal} 个：${excluded.map(([reason, count]) => `${reasons[reason] ?? reason} ${count}`).join('、')}`, `${excludedTotal} not included: ${excluded.map(([reason, count]) => `${reasons[reason] ?? reason} ${count}`).join(', ')}`) : '',
      row.newly_regular > 0 ? t(`${row.newly_regular} 个阶段会首次交给常规模型，之前由主力处理，没有常规模型的胜任证据`, `${row.newly_regular} stages would newly go to Regular; Primary handled them, so they carry no Regular competence evidence`) : '',
    ].filter(Boolean).join(t('；', '. '))}{t('。这是基于历史分配的估算，不是预测。', '. This estimates historical allocation; it is not a prediction.')}</p>}
  </div>;
}
