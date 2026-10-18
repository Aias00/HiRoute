import React, { useEffect, useRef, useState } from 'react';
import { formatMoney } from '../ui';
import { safeDiagnosticCode } from '../error-code';
import { observationRead } from './observation-client';
import { formatTokenCount } from './usage-presentation';

type Cost = { model_configuration_id: string | null; currency: string | null; valuation_kind: string | null; known_micros: number | null; unknown_contributions: number };
export type TimelineRequest = { request_id: string; started_at_ms: number; duration_ms: number | null; native_model: string | null; input_tokens: number | null; output_tokens: number | null; cache_read_tokens: number | null; cache_read_share: number | null; costs: Cost[]; outcome: string | null };
export type TimelineTurn = {
  turn_id: string; agent_turn_id: string | null; segment_id: string | null; started_at_ms: number; finished_at_ms: number | null;
  boundary: 'new_user_message' | 'context_rebuild' | 'unknown'; plan_id: string | null; plan_revision: number | null;
  branch_id: string | null; model_configuration_id: string | null; reasoning_profile_id: string | null; group: string | null;
  selection: { simple_probability: number | null; simple_threshold_millis: number | null; selection_reason: string; competence_trigger?: unknown } | null;
  tool_continuations: number; relays: { request_id: string; from_model: string | null; to_model: string | null }[];
  assessment_state: string; assessment: { score: number; partial: boolean; target_from_ordinal: number; target_through_ordinal: number } | null;
  competence_floor_millis: number | null; protected_turn_ids: string[]; status: string; requests: TimelineRequest[];
  cost_by_model: Cost[]; evidence_partial: boolean; content_digest: string;
};
type TimelinePage = { turns: TimelineTurn[]; watermark: string; complete: boolean };

export function RoutingTimeline({ session, language, active, refreshVersion, requestId, onSelectRequest, onOpenPerformance }: {
  session: string; language: 'zh' | 'en'; active: boolean; refreshVersion: number; requestId: string | null;
  onSelectRequest(request: TimelineRequest): void; onOpenPerformance(plan: string): void;
}) {
  const text = (zh: string, en: string) => language === 'zh' ? zh : en;
  const [turns, setTurns] = useState<TimelineTurn[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [retry, setRetry] = useState(0);
  const watermark = useRef<string | null>(null);
  const stored = useRef(new Map<string, TimelineTurn>());
  useEffect(() => { watermark.current = null; stored.current.clear(); setTurns([]); setSelected(null); setLoaded(false); }, [session, refreshVersion]);
  useEffect(() => {
    if (!active) return;
    const controller = new AbortController(); let inFlight = false; let timer = 0; let stopped = false;
    const schedule = (delay: number) => { window.clearTimeout(timer); if (!controller.signal.aborted) timer = window.setTimeout(() => void load(), delay); };
    async function load() {
      if (inFlight || stopped || controller.signal.aborted || document.visibilityState !== 'visible') return;
      inFlight = true;
      try {
        const page = await observationRead<TimelinePage>('routing_timeline', { session_id: session, after_watermark: watermark.current, limit: 50 }, controller.signal);
        if (controller.signal.aborted) return;
        let changed = false;
        for (const turn of page.turns) { if (stored.current.get(turn.turn_id)?.content_digest !== turn.content_digest) { stored.current.set(turn.turn_id, turn); changed = true; } }
        if (changed) setTurns([...stored.current.values()].sort((a, b) => a.started_at_ms - b.started_at_ms || a.turn_id.localeCompare(b.turn_id)));
        watermark.current = page.watermark; setError(''); setLoaded(true);
        schedule(page.complete ? 2000 : 0);
      } catch (e) {
        if (controller.signal.aborted) return;
        const code = safeDiagnosticCode(e, 'OBSERVATION_UNAVAILABLE');
        const failure = e as { observation_read?: { reason?: string; retry_after_ms?: number } };
        if (failure?.observation_read?.reason === 'reader_busy' || code === 'OBSERVATION_QUERY_BUSY') {
          schedule(Math.min(1000, Math.max(100, failure?.observation_read?.retry_after_ms ?? 100))); return;
        }
        stopped = code.toLowerCase().includes('scope_too_large');
        const stale = code.toLowerCase().includes('stale') && watermark.current !== null;
        if (stale) { watermark.current = null; stored.current.clear(); setTurns([]); setLoaded(false); }
        setError(code); if (!stopped) schedule(stale ? 0 : 2000);
      } finally { inFlight = false; }
    }
    const visible = () => { if (document.visibilityState === 'visible') void load(); else window.clearTimeout(timer); };
    const invalidate = () => { controller.abort(); stored.current.clear(); watermark.current = null; setTurns([]); setLoaded(false); setRetry(n => n + 1); };
    document.addEventListener('visibilitychange', visible); window.addEventListener('hiroute-content-invalidated', invalidate); void load();
    return () => { controller.abort(); window.clearTimeout(timer); document.removeEventListener('visibilitychange', visible); window.removeEventListener('hiroute-content-invalidated', invalidate); };
  }, [session, active, refreshVersion, retry]);
  useEffect(() => { const turn = turns.find(t => t.requests.some(r => r.request_id === requestId)); if (turn) setSelected(turn.turn_id); }, [requestId, turns]);
  const selectedTurn = turns.find(t => t.turn_id === selected) ?? turns[0];
  const boundary = (turn: TimelineTurn) => turn.boundary === 'new_user_message' ? text('新用户消息', 'New user message') : turn.boundary === 'context_rebuild' ? text('上下文重建', 'Context rebuilt') : text('边界未确认', 'Boundary unknown');
  const group = (turn: TimelineTurn) => turn.group === 'primary' ? text('主力', 'Primary') : turn.group === 'regular' ? text('常规', 'Regular') : text('分组未记录', 'Group unknown');
  const score = (turn: TimelineTurn) => turn.assessment ? `${text('阶段评分', 'Stage score')} ${turn.assessment.score.toFixed(2)}${turn.assessment.partial ? text(' · 部分评分', ' · Partial') : ''}` : turn.assessment_state === 'pending' ? text('等待评估', 'Awaiting assessment') : text('未评分', 'Unscored');
  const selectionReason = (reason: string | undefined) => ({simple_task:text('简单任务', 'Simple task'),complex_task:text('复杂任务', 'Complex task'),low_competence:text('低胜任度保护', 'Low-competence protection'),degree_unavailable:text('难度判断不可用', 'Difficulty unavailable'),single_group:text('单组分类', 'Single group'),decision_fallback:text('决策失败后回退', 'Decision fallback'),availability_relay:text('可用性接力', 'Availability relay'),heuristic:text('启发式判断', 'Heuristic judgment')} as Record<string,string>)[reason ?? ''] ?? text('选择原因未知', 'Selection reason unknown');
  const amount = (cost: Cost) => cost.known_micros === null || !cost.currency ? text('金额未知', 'Amount unknown') : `${formatMoney(cost.currency, (cost.known_micros / 1_000_000).toFixed(4))}${cost.unknown_contributions ? text(' + 未知部分', ' + unknown portion') : ''}`;
  return <section className="routing-timeline" aria-label={text('路由时间线', 'Routing timeline')}>
    <header className="routing-timeline-heading"><h3>{text('路由时间线', 'Routing timeline')}</h3><span className="muted">{text('可见时每 2 秒更新', 'Updates every 2 s while visible')}</span></header>
    {error && <div className="callout warn" role="alert" data-error-code={error}><span>{error.toLowerCase().includes('scope_too_large') ? text('这条会话超出完整读取范围，暂时无法显示时间线。', 'This session exceeds the complete-read limit; its timeline is unavailable.') : text('更新暂时不可用，已读取内容保留。', 'Updates are temporarily unavailable. Loaded records are retained.')}</span><button className="btn" onClick={() => setRetry(n => n + 1)}>{text('重试', 'Retry')}</button></div>}
    {!loaded && !error && <p role="status">{text('正在读取轮次…', 'Reading turns…')}</p>}
    {loaded && !turns.length && <p className="muted">{text('还没有可显示的轮次。', 'No recorded turns yet.')}</p>}
    <div className="routing-turn-list" aria-label={text('Agent 轮次', 'Agent turns')}>
      {turns.map((turn, index) => <button type="button" className={`routing-turn${selectedTurn?.turn_id === turn.turn_id ? ' selected' : ''}`} key={turn.turn_id} aria-current={selectedTurn?.turn_id === turn.turn_id ? 'true' : undefined} onClick={() => { setSelected(turn.turn_id); if (turn.requests[0]) onSelectRequest(turn.requests[0]); }}>
        <span className="routing-turn-top"><strong>{text('轮次', 'Turn')} {index + 1}</strong><span className="muted">{new Date(turn.started_at_ms).toLocaleTimeString(language === 'zh' ? 'zh-CN' : 'en')}</span></span>
        <span>{boundary(turn)} · {group(turn)}</span><span className="muted">{selectionReason(turn.selection?.selection_reason)}</span><strong>{turn.model_configuration_id ?? text('模型未记录', 'Model unknown')}</strong>
        <span className="muted">{turn.selection?.simple_probability == null ? text('简单概率未知', 'Simple probability unknown') : `p = ${turn.selection.simple_probability.toFixed(2)}`}{turn.selection?.simple_threshold_millis != null && ` · ${text('阈值', 'Threshold')} ${(turn.selection.simple_threshold_millis / 1000).toFixed(2)}`}</span>
        <span>{score(turn)}{turn.competence_floor_millis != null && ` · ${text('底线', 'Floor')} ${(turn.competence_floor_millis / 1000).toFixed(2)}`}</span>
        {turn.selection?.competence_trigger != null && <span className="badge warn no-dot">{text('胜任度保护', 'Competence protection')}</span>}
        {turn.protected_turn_ids.length > 0 && <span className="muted">{text('此阶段评分触发了后续保护', 'This stage score triggered later protection')}</span>}
        {turn.evidence_partial && <span className="badge warn no-dot">{text('证据不完整', 'Incomplete evidence')}</span>}
      </button>)}
    </div>
    {selectedTurn && <div className="routing-turn-detail">
      <div className="routing-timeline-heading"><h4>{text('所选轮次', 'Selected turn')}</h4>{selectedTurn.plan_id && <button className="btn btn-quiet" onClick={() => onOpenPerformance(selectedTurn.plan_id!)}>{text('查看 Plan 模型表现', 'View Plan model performance')}</button>}</div>
      <p className="muted">{selectedTurn.tool_continuations} {text('次工具续轮', 'tool continuations')} · {selectedTurn.relays.length} {text('次请求内模型接力', 'model relays within requests')}{selectedTurn.reasoning_profile_id && ` · ${selectedTurn.reasoning_profile_id}`} · {text('状态', 'Status')}: {({ running: text('运行中', 'Running'), completed: text('完成', 'Completed'), failed: text('失败', 'Failed'), interrupted: text('中断', 'Interrupted') } as Record<string, string>)[selectedTurn.status] ?? text('未知', 'Unknown')}</p>
      {selectedTurn.assessment && <p className="muted">{text('这是阶段评分，覆盖轮次', 'This stage assessment covers turns')} {selectedTurn.assessment.target_from_ordinal}–{selectedTurn.assessment.target_through_ordinal}{text('。', '.')}</p>}
      {selectedTurn.relays.map((relay, index) => <p key={index}>{relay.from_model ?? '?'} → {relay.to_model ?? '?'}</p>)}
      <div className="routing-turn-requests">{selectedTurn.requests.map((request, index) => <button className={`routing-request${request.request_id === requestId ? ' selected' : ''}`} type="button" key={request.request_id} onClick={() => onSelectRequest(request)}>
        <strong>{text('请求', 'Request')} {index + 1} · {request.native_model ?? text('模型未记录', 'Model unknown')}</strong>
        <span>{text('输入', 'Input')} {request.input_tokens === null ? '—' : formatTokenCount(request.input_tokens, language)} · {text('输出', 'Output')} {request.output_tokens === null ? '—' : formatTokenCount(request.output_tokens, language)} · {text('缓存读取', 'Cache read')} {request.cache_read_share === null ? text('未知', 'Unknown') : `${(request.cache_read_share * 100).toFixed(1)}%`} · {request.duration_ms === null ? text('耗时未知', 'Duration unknown') : `${(request.duration_ms / 1000).toFixed(2)} s`}</span>
        <span>{request.costs.map(amount).join(' · ') || text('未计入估算', 'Excluded from estimate')}</span>
      </button>)}</div>
      <div className="routing-turn-costs"><strong>{text('按模型估算', 'Estimates by model')}</strong>{selectedTurn.cost_by_model.map((cost, index) => <p key={index}>{cost.model_configuration_id ?? text('模型未知', 'Unknown model')} · {cost.valuation_kind === 'api_equivalent' ? text('API 等价值', 'API-equivalent value') : text('估算调用成本', 'Estimated call cost')}: {amount(cost)}</p>)}</div>
    </div>}
  </section>;
}
