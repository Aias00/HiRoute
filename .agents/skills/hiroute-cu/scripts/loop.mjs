// Portions adapted from Sac-Y/Jev-cu; Copyright (c) 2026 Sac-Y.
// MIT notice: ../references/jev-LICENSE.txt
// Internal current contract; no compatibility layer for the external jev-cu API.
import {actions, parseAX, stateKey} from './ax.mjs';
export {createCuaDriver, createSkyDriver, parseAX} from './ax.mjs';

export function createBudget({maxSteps = 8, maxCalls = 10, timeoutMs = 45000} = {}) {
  if (![maxSteps, maxCalls, timeoutMs].every(n => Number.isSafeInteger(n) && n > 0)) {
    throw new Error('invalid_budget');
  }
  return {maxSteps, maxCalls, deadlineAt: Date.now() + timeoutMs,
    stepsUsed: 0, callsUsed: 0, busy: false, needsReconcile: false};
}

const sensitive = /删除|移除|清空|发送|提交|发布|回复|支付|购买|订阅|授权|权限|登录|密码|验证码|上传|分享|安装|安全设置|delete|remove|send|submit|publish|reply|pay|purchase|subscribe|permission|login|password|captcha|upload|share|install/i;
const probability = n => Number.isFinite(n) && n >= 0 && n <= 1;

export async function runTask({driver, appName, goal, scenario, budget,
  dryRun = true, select, decide, signal}) {
  const started = Date.now();
  const events = [];
  const observations = [];
  const assessments = [];
  const mode = typeof select === 'function' ? 'batch' : 'jev';
  const finish = (status, reason, verified = false) => ({status, reason, verified, mode, goal, appName,
    stepsUsed: budget?.stepsUsed ?? 0, callsUsed: budget?.callsUsed ?? 0,
    needsReconcile: budget?.needsReconcile ?? false, elapsedMs: Date.now() - started, events, observations, assessments});
  // Guards and external-data projection are required, even for dry-run.
  if (!driver || typeof driver.observe !== 'function' || typeof driver.execute !== 'function'
    || typeof appName !== 'string' || !appName || typeof goal !== 'string' || !goal
    || !scenario || !['project', 'beforeAction', 'afterAction', 'verify'].every(k => typeof scenario[k] === 'function')
    || (scenario.review !== undefined && typeof scenario.review !== 'function')
    || !Array.isArray(scenario.allowedActions) || !scenario.allowedActions.length
    || scenario.allowedActions.some(a => !actions.includes(a))
    || (select !== undefined && typeof select !== 'function')
    || (decide !== undefined && typeof decide !== 'function')
    || (typeof select === 'function') === (typeof decide === 'function')
    || !budget || ![budget.maxSteps, budget.maxCalls].every(n => Number.isSafeInteger(n) && n > 0)
    || ![budget.stepsUsed, budget.callsUsed].every(n => Number.isSafeInteger(n) && n >= 0)
    || !Number.isFinite(budget.deadlineAt)) return finish('escalate', 'invalid_contract');
  if (mode === 'jev' && scenario.allowedActions.some(a => !['click_element', 'scroll'].includes(a))) {
    return finish('escalate', 'jev_read_only_required');
  }
  if (budget.busy) return finish('escalate', 'budget_in_use');
  if (budget.needsReconcile) return finish('escalate', 'reconciliation_required');
  budget.busy = true;
  const controller = new AbortController();
  const abort = () => controller.abort();
  signal?.addEventListener('abort', abort, {once: true});
  if (signal?.aborted) abort();
  const timer = setTimeout(abort, Math.max(0, budget.deadlineAt - Date.now()));
  let phase = 'observe';
  let actionStarted = false;
  let pending;
  // Local, unredacted observations by explicit user choice. Never pass these
  // records to the selector/API or print/persist them implicitly.
  async function observe() {
    const ax = await driver.observe();
    if (typeof ax === 'string') observations.push({phase, ax});
    return ax;
  }
  // Native actions are not necessarily cancellable. Hold the shared budget busy
  // until a timed-out operation settles; never allow overlapping continuation.
  async function bounded(fn) {
    if (controller.signal.aborted || Date.now() >= budget.deadlineAt) throw new Error('deadline');
    pending = Promise.resolve().then(fn);
    let listener;
    const cancelled = new Promise((_, reject) => {
      listener = () => reject(new Error('cancelled'));
      controller.signal.addEventListener('abort', listener, {once: true});
    });
    try {
      const result = await Promise.race([pending, cancelled]);
      pending = undefined;
      if (controller.signal.aborted || Date.now() >= budget.deadlineAt) throw new Error('deadline');
      return result;
    } finally { controller.signal.removeEventListener('abort', listener); }
  }
  try {
    let ax = await bounded(observe);
    if (typeof ax !== 'string') return finish('escalate', 'invalid_observation');
    const baseline = ax;
    let noProgress = 0;
    const screening = mode === 'jev' && typeof scenario.review === 'function';
    let previous = null;
    while (true) {
      phase = 'verify';
      const verified = await bounded(() => scenario.verify({ax, baseline})) === true;
      if (verified && !screening) return finish('done', 'verified', true);
      if (budget.stepsUsed >= budget.maxSteps && !screening) return finish('max_steps', 'step_budget');
      if (mode === 'jev' && budget.callsUsed >= budget.maxCalls) return finish('escalate', 'call_budget');
      const assessOnly = screening && (verified || noProgress >= 2 || budget.stepsUsed >= budget.maxSteps);
      phase = 'project';
      const projection = assessOnly ? {context: 'Final observation; do not select an action.', candidates: []}
        : await bounded(() => scenario.project({ax, baseline}));
      const actual = parseAX(ax);
      if (!projection || typeof projection.context !== 'string' || projection.context.length > 1500
        || !Array.isArray(projection.candidates) || (!screening && !projection.candidates.length) || projection.candidates.length > 40
        || new Set(projection.candidates.map(c => c.index)).size !== projection.candidates.length
        || projection.candidates.some(c => !Number.isSafeInteger(c.index) || typeof c.label !== 'string'
          || c.label.length > 160 || !actual.some(a => a.index === c.index && a.role === c.role))) {
        return finish('escalate', 'invalid_projection');
      }
      // The host must positively identify each candidate as a read-only
      // operation. A benign label alone cannot establish that fact.
      if (mode === 'jev' && projection.candidates.some(c => c.readOnly !== true)) {
        return finish('escalate', 'read_only_scope_required');
      }
      // Copy only these fields: caller-provided raw AX on a candidate is never sent.
      const candidates = projection.candidates.map(({index, role, label}) => Object.freeze({index, role, label}));
      let review;
      if (screening) {
        phase = 'review_projection';
        const supplied = await bounded(() => scenario.review({ax, baseline}));
        if (!supplied || typeof supplied.expectation !== 'string' || !supplied.expectation.trim()
          || supplied.expectation.length > 1500 || !Array.isArray(supplied.facts)
          || !supplied.facts.length || supplied.facts.length > 12
          || supplied.facts.some(f => typeof f !== 'string' || !f.trim() || f.length > 500)) {
          return finish('escalate', 'invalid_review_projection');
        }
        // Only this explicit projection leaves the host. Never spread arbitrary fields.
        review = {expectation: supplied.expectation, facts: [...supplied.facts], previous};
      }
      const event = {step: budget.stepsUsed + 1, context: projection.context,
        execution: 'not_started', beforeCheck: null, afterCheck: null};
      events.push(event);
      phase = mode === 'batch' ? 'select' : 'decide';
      if (mode === 'jev') budget.callsUsed++;
      const decision = await bounded(() => (select ?? decide)({goal, app: appName,
        candidates, context: projection.context, allowedActions: [...scenario.allowedActions],
        ...(review ? {review} : {}), signal: controller.signal}));
      if (mode === 'jev') {
        if (Number.isFinite(decision?.latencyMs) && decision.latencyMs >= 0) event.latencyMs = decision.latencyMs;
        if (decision?.usage && [decision.usage.input_tokens, decision.usage.output_tokens]
          .every(n => Number.isSafeInteger(n) && n >= 0)) {
          event.usage = {input_tokens: decision.usage.input_tokens, output_tokens: decision.usage.output_tokens};
        }
      }
      if (screening) {
        const a = decision?.assessment;
        const evidenceIndex = typeof a?.evidence === 'string' && /^f\d+$/.test(a.evidence)
          ? Number(a.evidence.slice(1)) : -1;
        if (!a || !['normal', 'loading', 'suspected_issue', 'unknown'].includes(a.kind)
          || !probability(a.confidence) || (a.evidence !== 'none' && !(evidenceIndex >= 0 && evidenceIndex < review.facts.length))) {
          return finish('escalate', 'invalid_assessment');
        }
        assessments.push({observation: observations.length - 1, afterSteps: budget.stepsUsed,
          expectation: review.expectation, facts: [...review.facts], previous,
          kind: a.kind, confidence: a.confidence, evidence: a.evidence,
          evidenceText: evidenceIndex >= 0 ? review.facts[evidenceIndex] : null});
        if (a.confidence < 0.7) return finish('escalate', 'assessment_low_confidence');
        if ((a.kind === 'suspected_issue') !== (a.evidence !== 'none')) {
          return finish('escalate', 'assessment_inconsistent');
        }
        if (a.kind !== 'normal') return finish('escalate', `assessment_${a.kind}`);
        if (verified) {
          phase = 'refresh';
          if (await bounded(observe) !== ax) return finish('escalate', 'observation_changed');
          return finish('done', 'verified', true);
        }
        if (noProgress >= 2) return finish('escalate', 'no_progress');
        if (budget.stepsUsed >= budget.maxSteps) return finish('max_steps', 'step_budget');
        if (!candidates.length) return finish('escalate', 'selection_unresolved');
      }
      if (mode === 'batch' && !decision) return finish('escalate', 'selection_unresolved');
      if (mode === 'jev' && (!decision || ![decision.confidence, decision.risk, decision.done].every(probability))) {
        return finish('escalate', 'invalid_decision');
      }
      if (mode === 'jev' && decision.done >= 0.9) return finish('escalate', 'unverified_done');
      if (!scenario.allowedActions.includes(decision.action)) return finish('escalate', 'action_out_of_scope');
      const target = candidates.find(c => c.index === decision.targetIndex);
      if (!target) return finish('escalate', 'target_out_of_scope');
      // Semantic breadcrumbs accompany the full local observations; arbitrary
      // model response fields and API authentication are not part of the trace.
      Object.assign(event, {kind: decision.action, target: {...target}});
      const element = actual.find(c => c.index === target.index);
      if (sensitive.test(element.label) || (mode === 'jev' && decision.risk >= 0.2)) return finish('confirm', 'sensitive_action');
      if (mode === 'jev' && decision.confidence < 0.5) return finish('escalate', 'low_confidence');
      phase = 'parameters';
      const supplied = scenario.resources ? await bounded(() => scenario.resources({ax, element, kind: decision.action})) : {};
      const action = Object.freeze({kind: decision.action, targetIndex: target.index,
        ...(decision.action === 'set_value' ? {text: supplied?.text} : {}),
        ...(decision.action === 'scroll' ? {direction: supplied?.direction} : {})});
      if ((action.kind === 'set_value' && typeof action.text !== 'string')
        || (action.kind === 'scroll' && !['up', 'down', 'left', 'right'].includes(action.direction))) {
        return finish('escalate', 'invalid_parameters');
      }
      event.action = {...action};
      // Observe again after selection; never act on stale indices in either mode.
      phase = 'refresh';
      const fresh = await bounded(observe);
      if (fresh !== ax) return finish('escalate', 'observation_changed');
      phase = 'before_action';
      event.beforeCheck = await bounded(() => scenario.beforeAction({ax, baseline, action, element})) === true;
      if (!event.beforeCheck) {
        return finish('escalate', 'before_action_rejected');
      }
      if (dryRun) return finish('dry_run', 'guards_passed');
      phase = 'execute';
      actionStarted = true;
      budget.stepsUsed++;
      event.execution = 'started';
      await bounded(() => driver.execute(action));
      event.execution = 'returned';
      if (screening) previous = {facts: [...review.facts], action: {kind: action.kind, target: target.label}};
      phase = 'observe_after';
      const after = await bounded(observe);
      if (typeof after !== 'string') throw new Error('invalid_observation');
      phase = 'after_action';
      event.afterCheck = await bounded(() => scenario.afterAction({before: ax, ax: after, baseline, action, element})) === true;
      if (!event.afterCheck) {
        budget.needsReconcile = true;
        return finish('escalate', 'after_action_rejected');
      }
      actionStarted = false;
      const key = scenario.progressKey ?? stateKey;
      phase = 'progress';
      const beforeKey = await bounded(() => key(ax));
      const afterKey = await bounded(() => key(after));
      noProgress = beforeKey === afterKey ? noProgress + 1 : 0;
      ax = after;
      // Goal verification wins over a coarse progress key, but never over guards.
      if (!screening) {
        phase = 'verify';
        if (await bounded(() => scenario.verify({ax, baseline})) === true) return finish('done', 'verified', true);
        if (noProgress >= 2) return finish('escalate', 'no_progress');
      }
    }
  } catch (error) {
    if (actionStarted) budget.needsReconcile = true;
    const safeCode = /^jev_(?:http_\d{3}|key_missing|invalid_response|cancelled_or_timeout)$/.test(error?.message ?? '')
      ? error.message : `${phase}_failed`;
    const reason = controller.signal.aborted || Date.now() >= budget.deadlineAt
      ? (signal?.aborted ? 'cancelled' : 'deadline') : safeCode;
    // Never return exception text: drivers/guards can include secrets in errors.
    return finish('escalate', reason);
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', abort);
    controller.abort();
    if (pending) pending.then(() => { budget.busy = false; }, () => { budget.busy = false; });
    else budget.busy = false;
  }
}
