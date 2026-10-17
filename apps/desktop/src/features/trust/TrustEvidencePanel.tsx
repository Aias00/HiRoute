import { ContentExcerpt } from '../Sessions';
import { agentDisplayName } from '../agents/ecosystems';
import { useEffect, useState } from 'react';
import { Disclosure } from '../../ui';
import { observationRead } from '../observation-client';
import { safeDiagnosticCode } from '../../error-code';
import { trustLevelLabel } from './levels';
import type { TrustReport, TrustRow, TrustBranch } from './types';
import './trust.css';

export function TrustEvidencePanel({ planId, revision, fromMs, toMs, active, language, names, onSelect, onThreshold, onDivision, onOpenAgent, onOpenSession }: {
  planId: string; revision: 'active' | 'all' | number; fromMs?: number; toMs: number; active: boolean; language: 'zh' | 'en'; names: Map<string, string>;
  onOpenAgent?(agent: string): void; onOpenSession?(session: string): void;
  onSelect(row: TrustRow, groupByExecutor: boolean): void; onThreshold?(branch: string, threshold: number): void; onDivision?(branch: string): void;
}) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  const [groupBy, setGroupBy] = useState<'model' | 'executor'>('model');
  const [report, setReport] = useState<TrustReport | null>(null);
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  const [sessionLimit, setSessionLimit] = useState(12);
  useEffect(() => {
    if (!active) return;
    let current = true;
    setReport(null); setError('');
    observationRead<TrustReport>('plan_trust', { plan_id: planId, revision, from_ms: fromMs, to_ms: toMs, group_by: groupBy })
      .then(value => { if (current) setReport(value); })
      .catch(cause => { if (current) setError(safeDiagnosticCode(cause, 'OBSERVATION_UNAVAILABLE')); });
    return () => { current = false; };
  }, [planId, revision, fromMs, toMs, active, groupBy, retry]);
  const percent = (value: number | null) => value == null ? '—' : `${Math.round(value * 100)}%`;
  function branchSuggestion(branch: TrustBranch) {
    const suggestion = branch.suggestion;
    if (suggestion.kind === 'step_up' || suggestion.kind === 'step_down') {
      const replay = branch.replay.thresholds.find(row => row.threshold_millis === suggestion.threshold_millis);
      return <><p>{t(`可以考虑${suggestion.kind === 'step_up' ? '升' : '降'}到「${trustLevelLabel(suggestion.threshold_millis, language)}」。`, `Consider ${suggestion.kind === 'step_up' ? 'stepping up' : 'stepping down'} to ${trustLevelLabel(suggestion.threshold_millis, language)}.`)}</p>{replay && <p>{t(`历史回放中，常规组占比为 ${percent(replay.regular_share)}，${replay.protected_as_primary} 个阶段仍由主力保护。`, `Historical replay assigns ${percent(replay.regular_share)} to Regular; ${replay.protected_as_primary} stages remain protected by Primary.`)}{replay.newly_regular > 0 && t(` ${replay.newly_regular} 个阶段将首次交给常规组，历史由主力处理，没有常规胜任证据。`, ` ${replay.newly_regular} stages would newly go to Regular. Primary handled them, so they carry no Regular competence evidence.`)}</p>}{onThreshold && <button type="button" className="btn" onClick={() => onThreshold(branch.branch_id, suggestion.threshold_millis)}>{t('查看档位调整草稿', 'Review level change')}</button>}</>;
    }
    if (suggestion.kind === 'explore_division') return <><p>{t('当前证据支持探索模型和执行 Agent 分工。', 'Current evidence supports exploring model and execution-agent division.')}</p>{onDivision && <button type="button" className="btn" onClick={() => onDivision(branch.branch_id)}>{t('查看分工草稿', 'Review division draft')}</button>}</>;
    if (suggestion.kind === 'return_to_previous_routing') return <><p>{t('常规组胜任率偏低，可以审阅已确认存在的历史映射。', 'Regular competence is low. Review an available historical mapping.')}</p>{(report?.previous_routing ?? []).map(target => <button className="btn" key={`${target.agent}/${target.presets.join(',')}`} disabled={!onOpenAgent} onClick={() => onOpenAgent?.(target.agent)}>{t('查看之前的路由', 'Review previous routing')} · {agentDisplayName(target.agent, language)} · {target.presets.join(', ')}</button>)}</>;
    if (suggestion.kind === 'change_regular_model') return <p>{t('常规组胜任率偏低，可以调整常规模型。尚未确认有可恢复的历史映射。', 'Regular competence is low. Consider changing its model; no recoverable historical mapping is confirmed.')}</p>;
    return <p>{suggestion.kind === 'keep' && suggestion.missing.length ? t('证据尚不足以建议提高信任。保持当前档位即可。', 'Evidence is not sufficient to recommend raising trust. Keeping this level is a valid choice.') : t('建议保持当前档位。', 'Keeping the current level is a valid choice.')}</p>;
  }
  return <section className="trust-evidence" aria-label={t('信任证据', 'Trust evidence')}>
    <div className="quality-toolbar"><h4>{t('胜任率与评分覆盖率', 'Competence and scoring coverage')}</h4><label><span className="sr-only">{t('观察分组', 'Evidence grouping')}</span><select className="select" value={groupBy} onChange={e => setGroupBy(e.target.value as 'model' | 'executor')}><option value="model">{t('按模型', 'By model')}</option><option value="executor">{t('按执行 Agent', 'By execution agent')}</option></select></label></div>
    {error && <div className="callout warn" role="alert" data-error-code={error}><span>{error.toLowerCase().includes('scope_too_large') ? t('证据超过读取上限。建议需要本修订版的全部保留记录；缩短显示时间不一定能减少建议计算范围。', 'Evidence exceeds the read limit. Suggestions need the full retained revision; narrowing the displayed window may not reduce that scope.') : t('证据暂时无法读取，当前档位未改变。', 'Evidence is unavailable. Your trust level has not changed.')}</span><button className="btn" type="button" onClick={() => setRetry(n => n + 1)}>{t('重试', 'Retry')}</button></div>}
    {!report && !error && <p role="status">{t('正在读取完整范围的证据…', 'Reading the complete evidence scope…')}</p>}
    {report && <>
      {!report.rows.length && <p>{t('当前范围尚无执行证据。零样本不代表模型胜任。', 'No execution evidence in this scope yet. Zero samples do not establish competence.')}</p>}
      {report.rows.map((row, index) => <article className="trust-evidence-row" key={index}>
        <strong>{row.attribution !== 'single' ? t(row.attribution === 'mixed' ? '多个执行模型' : '执行模型未确认', row.attribution === 'mixed' ? 'Multiple execution models' : 'Execution model unknown') : names.get(row.model_configuration_id ?? '') ?? row.model_configuration_id ?? t('未知模型', 'Unknown model')} · {row.group === 'regular' ? t('常规', 'Regular') : row.group === 'primary' ? t('主力', 'Primary') : t('执行组未知', 'Unknown group')}{row.reasoning_profile_id ? ` · ${row.reasoning_profile_id}` : ''}{groupBy === 'executor' ? ` · ${row.executor ?? t('执行 Agent 未知', 'Unknown executor')}` : ''}</strong>
        <div className="trust-metrics"><span>{t('胜任率', 'Competence rate')} <strong>{percent(row.competence_rate)}</strong></span><span>{t('评分覆盖率', 'Scoring coverage')} <strong>{percent(row.scoring_coverage)}</strong></span><span>{t('执行占比', 'Execution share')} <strong>{percent(row.execution_share)}</strong></span></div>
        <p>{t(`${row.counts.executed} 次执行 · ${row.counts.complete} 完整评分 · ${row.counts.partial} 部分评分 · ${row.counts.awaiting_assessment} 等待评分 · ${row.counts.unscored} 未评分`, `${row.counts.executed} executed · ${row.counts.complete} complete · ${row.counts.partial} partial · ${row.counts.awaiting_assessment} awaiting assessment · ${row.counts.unscored} unscored`)}</p>
        <p>{t(`${row.protections.length} 次主力保护 · ${row.availability_relays} 次可用性接力`, `${row.protections.length} Primary protections · ${row.availability_relays} availability relays`)}</p>
        {['rolling-alias', 'dynamic-route', 'unknown'].includes(row.identity_stability) && <p className="field-help">{t('供应商可能在不改变模型 ID、目录或路由的情况下更新模型；这些证据可能混合多个版本，实际执行版本未经确认。', 'The provider can update the model behind this ID without a catalog or Plan change. Evidence may mix versions; the executed version is unverified.')}</p>}
        {row.duration_p50_ms !== null && <p>{t('已记录请求耗时', 'Recorded request duration')} p50 {row.duration_p50_ms} ms · p90 {row.duration_p90_ms ?? '—'} ms{row.output_tokens_per_second != null ? ` · ${row.output_tokens_per_second.toFixed(1)} tokens/s` : ''}</p>}

        <button className="btn btn-quiet" type="button" onClick={() => onSelect(row, groupBy === 'executor')}>{t('查看执行阶段', 'View execution stages')}</button>
      </article>)}
      {groupBy === 'executor' && (report.executors ?? []).map(group => <article className="trust-evidence-row" key={group.executor}><strong>{group.executor}</strong><p>{t(`本范围新提交 ${group.task_outcomes.submitted ?? 0} 个任务 · 完成 ${group.task_outcomes.succeeded ?? 0} · 失败 ${group.task_outcomes.failed ?? 0} · 取消 ${group.task_outcomes.cancelled ?? 0} · 运行中 ${group.task_outcomes.running ?? 0} · 未知 ${group.task_outcomes.unknown ?? 0} · Continue ${group.task_outcomes.continued ?? 0}`, `Tasks submitted in this scope: ${group.task_outcomes.submitted ?? 0} · succeeded ${group.task_outcomes.succeeded ?? 0} · failed ${group.task_outcomes.failed ?? 0} · cancelled ${group.task_outcomes.cancelled ?? 0} · running ${group.task_outcomes.running ?? 0} · unknown ${group.task_outcomes.unknown ?? 0} · continued ${group.task_outcomes.continued ?? 0}`)}</p><p className="field-help">{t('按任务首次提交时间计数，显示其最新运行状态，也包含尚未产生模型执行阶段的任务。', 'Counted by the task’s initial submission time, using its latest run status, including tasks with no model execution stages.')}</p></article>)}
      {groupBy === 'executor' && <p className="field-help">{t('各组处理的任务不同，不作排名。Agent 名称不代表安装版本；Worker 完成不等于交付验收或胜任，完成但无评分仍算未评分。', 'Groups handled different tasks and are not ranked. Agent names do not identify installed versions. Worker completion is not delivery acceptance or competence; completion without a score stays unscored.')}</p>}
      {report.announcements.map(a => <p className="field-help" key={`${a.model_configuration_id}:${a.included_on}`}>{t(`更新公告 ${a.included_on}：之前 ${a.before}，公告当日 ${a.announcement_day}，之后 ${a.after}。旧评分不验证更新；公告日期不代表供应商部署时间。`, `Update announcement ${a.included_on}: ${a.before} before, ${a.announcement_day} on the day, ${a.after} after. Earlier scores do not validate the update; this date does not establish provider deployment timing.`)}</p>)}
      <Disclosure label={t('档位建议与历史回放', 'Level suggestions and historical replay')} language={language}>
        <p className="field-help">{t('建议只针对当前发布修订版。升档至少需要 30 个完整评分、胜任率 85%、评分覆盖率 60% 和 3 个评分会话；低于 70% 时优先建议降档。', 'Suggestions apply only to the active revision. Raising trust requires 30 complete scores, 85% competence, 60% coverage and 3 scored sessions. A rate below 70% takes priority for stepping down.')}</p>
        {report.branches.map(branch => <div key={branch.branch_id}><strong>{trustLevelLabel(branch.threshold_millis, language)} · {branch.branch_id}</strong><p>{t(`全部历史：${branch.full_history.complete}/${branch.full_history.executed} 完整评分；升档证据：${branch.upward_cohort.complete}/${branch.upward_cohort.executed}，来自 ${branch.upward_cohort.sessions} 个评分会话。`, `Full history: ${branch.full_history.complete}/${branch.full_history.executed} complete; upward cohort: ${branch.upward_cohort.complete}/${branch.upward_cohort.executed}, from ${branch.upward_cohort.sessions} scored sessions.`)}</p>{branchSuggestion(branch)}</div>)}
        <p className="field-help">{t('这是基于你历史记录的估算，不是预测。建议不会自动修改或发布路由。', 'Estimate from your history, not a prediction. Suggestions do not automatically change or publish routing.')}</p>
      </Disclosure>
      <p className="field-help">{t('成本对比暂不可用：比较基准尚未接入。', 'Cost comparison is unavailable: the comparison basis is not available yet.')}</p>
      <p className="field-help">{t(`证据来自 ${report.session_total} 个会话。`, `Evidence comes from ${report.session_total} sessions.`)}</p>
      {report.sessions.length > 0 && <Disclosure label={t('贡献证据的会话', 'Contributing sessions')} language={language}><div className="native-list">{report.sessions.slice(0, sessionLimit).map(session => <button className="list-row" key={session.session_id} disabled={!onOpenSession} onClick={() => onOpenSession?.(session.session_id)}><span className="row-main"><ContentExcerpt session={session.session_id} language={language} fallback={t('会话记录', 'Session')} limit={64} /></span><span>{session.stages} {t('个阶段', 'stages')}</span></button>)}</div>{sessionLimit < report.sessions.length && <button className="btn" onClick={() => setSessionLimit(n => n + 12)}>{t('查看更多', 'Show more')}</button>}</Disclosure>}
      {onDivision && report.branches.length > 0 && <Disclosure label={t('尝试任务分工', 'Explore task division')} language={language}><p className="field-help">{t('可以随时查看分工草稿；打开草稿不代表已有证据支持扩大试用范围。', 'You can review division drafts at any time. Opening a draft does not establish evidence for broader use.')}</p>{report.branches.map(branch => <button className="btn" key={branch.branch_id} onClick={() => onDivision(branch.branch_id)}>{t('查看分工草稿', 'Review division draft')} · {branch.branch_id}</button>)}</Disclosure>}

    </>}
  </section>;
}
