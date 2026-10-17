import type { TrustSuggestionsRead } from './use-trust-suggestions';
import { trustLevelLabel } from './levels';

export function TrustAttention({ read, language, names, onOpen, onAct }: { read: TrustSuggestionsRead; language: 'zh' | 'en'; names: Record<string, string>; onOpen(planId: string): void; onAct?(planId: string, branchId: string): void }) {
  const text = (zh: string, en: string) => language === 'zh' ? zh : en;
  return <section className="surface trust-attention" aria-label={text('可选的路由调整', 'Optional routing adjustments')}>
    <header className="surface-header"><h2>{text('需要关注', 'Needs your attention')}</h2><span className="muted">{text('保持当前档位也是正常选择', 'Keeping your current level is a valid choice')}</span></header>
    {read.status === 'ready' && !read.plans.length && <p>{text('当前没有路由调整建议。保持当前档位即可。', 'No routing adjustments are suggested. Keeping the current level is a valid choice.')}</p>}
    {read.status === 'loading' && !read.plans.length && <p role="status">{text('正在读取路由证据…', 'Reading routing evidence…')}</p>}
    {read.status === 'error' && <div className="callout warn" role="alert" data-error-code={read.error}><span>{text('建议暂时无法刷新。已有建议可能过时，请重试后再决定。', 'Suggestions could not refresh. Loaded suggestions may be outdated; retry before deciding.')}</span><button className="btn" onClick={read.refresh}>{text('重试', 'Retry')}</button></div>}
    {read.plans.flatMap(plan => plan.branches.filter(branch => branch.suggestion.kind !== 'keep').map(branch => {
      const suggestion = branch.suggestion;
      const cohort = suggestion.kind === 'step_up' || suggestion.kind === 'explore_division' ? branch.upward_cohort : branch.full_history;
      const copy = suggestion.kind === 'step_up' ? text('可以尝试升到', 'You can try stepping up to') : suggestion.kind === 'step_down' ? text('可以退回', 'You can step down to') : suggestion.kind === 'explore_division' ? text('可以尝试按任务分工', 'You can explore division by task') : suggestion.kind === 'return_to_previous_routing' ? text('可以回到之前的路由', 'You can return to previous routing') : text('可以更换常规模型', 'You can change the Regular model');
      const rows = plan.rows.filter(r => r.branch_id === branch.branch_id && r.group === 'regular');
      const protectedCount = new Set(rows.flatMap(r => r.protections.map(p => p.segment_id))).size;
      const replay = 'threshold_millis' in suggestion ? branch.replay.thresholds.find(r => r.threshold_millis === suggestion.threshold_millis) : undefined;
      return <article key={`${plan.plan_id}/${branch.branch_id}`} className="trust-attention-item">
        <div><strong>{names[plan.plan_id] ?? plan.plan_id} · {copy}{'threshold_millis' in suggestion && ` ${trustLevelLabel(suggestion.threshold_millis, language)}`}</strong>
          <p className="muted">{text('当前档位', 'Current level')}: {trustLevelLabel(branch.threshold_millis, language)} · {text('发布修订版', 'Published revision')} {plan.active_revision}{(suggestion.kind === 'step_up' || suggestion.kind === 'explore_division') && ` · ${text('升档证据自', 'Upward evidence since')} ${new Date(branch.upward_from_ms).toLocaleDateString(language === 'zh' ? 'zh-CN' : 'en-US')}`}</p>
          <p>{cohort.complete} {text('完整评分', 'complete scores')} · {cohort.executed ? Math.round(cohort.complete / cohort.executed * 100) : 0}% {text('覆盖率', 'coverage')} · {cohort.sessions} {text('个会话', 'sessions')} · {cohort.complete ? Math.round(cohort.meets_floor / cohort.complete * 100) : '—'}% {text('胜任率', 'competence')} · {protectedCount} {text('次保护', 'protections')}</p>
          {replay && <p>{text('历史回放估计的常规占比', 'Estimated Regular share from history')}: {replay.regular_share === null ? '—' : `${Math.round(replay.regular_share * 100)}%`} · {text('不代表未来表现', 'Not a prediction')}</p>}
          {rows.some(r => ['rolling-alias', 'dynamic-route', 'unknown'].includes(r.identity_stability)) && <p className="muted">{text('供应商可能更新同名模型；这些证据可能混合多个版本。', 'The provider may update this model alias; evidence may mix versions.')}</p>}
          {plan.announcements.length > 0 && <p className="muted">{text('升档仅参考最新更新公告日之后的证据，实际执行版本未确认。', 'Upward suggestions use evidence after the latest update announcement day. The executed version is unverified.')}</p>}
        </div>
        <div className="actions">{onAct && <button className="btn btn-primary" disabled={read.status !== 'ready'} onClick={() => onAct(plan.plan_id, branch.branch_id)}>{suggestion.kind === 'explore_division' ? text('查看分工草稿', 'Review division draft') : suggestion.kind === 'return_to_previous_routing' ? text('查看之前的路由', 'Review previous routing') : text('查看调整草稿', 'Review routing change')}</button>}<button className="btn" disabled={read.status !== 'ready'} onClick={() => onOpen(plan.plan_id)}>{text('查看证据与选项', 'Review evidence and options')}</button></div>
      </article>;
    }))}
  </section>;
}
