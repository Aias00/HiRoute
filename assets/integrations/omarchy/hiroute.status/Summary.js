.pragma library
// Strict reader for `hiroute integrations omarchy summary`. It accepts only the bounded
// hiroute.omarchy-summary/v1 shape; anything else is unavailable, never stale or partial.

var SCHEMA = "hiroute.omarchy-summary/v1"
var MAX_BYTES = 65536
var MAX_IDS = 20
var WINDOW_MS = 7 * 24 * 60 * 60 * 1000
var PRESETS = { trial: 900, familiar: 800, trusted: 650, hands_off: 500 }
var SLOTS = { codex: ["default"], "claude-code": ["opus", "sonnet", "haiku"] }
var HEADLINES = ["no_routed_requests", "no_subscription_in_period", "subscription_usage_unknown", "no_comparable_usage", "share"]

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value)
}

function hasExactly(value, required, optional) {
  if (!isObject(value)) return false
  var allowed = required.concat(optional || [])
  for (var i = 0; i < required.length; i++)
    if (!Object.prototype.hasOwnProperty.call(value, required[i])) return false
  for (var key in value)
    if (allowed.indexOf(key) < 0) return false
  return true
}

function isInt(value, min, max) {
  return typeof value === "number" && Math.floor(value) === value && value >= min && value <= max
}

function isCode(value) {
  return typeof value === "string" && /^[a-z0-9_]{1,80}$/.test(value)
}

function isSuggestionId(value) {
  return typeof value === "string" && /^[A-Za-z0-9._-]{1,128}$/.test(value)
}

function isPlanId(value) {
  return typeof value === "string" && value.length <= 256 && /^[A-Za-z0-9._\/-]+$/.test(value)
    && value[0] !== "/" && value[value.length - 1] !== "/"
    && value.indexOf("//") < 0 && value.indexOf("..") < 0
}

function hasControl(value) {
  for (var i = 0; i < value.length; i++) {
    var c = value.charCodeAt(i)
    if (c < 0x20 || (c >= 0x7f && c <= 0x9f)) return true
  }
  return false
}

function isLabel(value, maxChars) {
  return typeof value === "string" && value.length > 0 && Array.from(value).length <= maxChars && !hasControl(value)
}

function isSessionId(value) {
  // Passed as one argument to the timeline command; never an option.
  return typeof value === "string" && value.length > 0 && value.length <= 256 && value[0] !== "-" && !hasControl(value)
}

function utf8Bytes(text) {
  var bytes = 0
  for (var i = 0; i < text.length; i++) {
    var c = text.charCodeAt(i)
    if (c < 0x80) bytes += 1
    else if (c < 0x800) bytes += 2
    else if (c >= 0xd800 && c <= 0xdbff) { bytes += 4; i++ }
    else bytes += 3
  }
  return bytes
}

function validTrust(plan) {
  var state = plan.trust_state, level = plan.trust_level, threshold = plan.threshold_millis
  if (state === "not_applicable" || state === "branch_specific") return level === null && threshold === null
  if (state !== "uniform" || !isInt(threshold, 0, 1000)) return false
  if (level === "custom") {
    for (var preset in PRESETS) if (PRESETS[preset] === threshold) return false
    return true
  }
  return Object.prototype.hasOwnProperty.call(PRESETS, level) && PRESETS[level] === threshold
}

function validSlotPlans(agent) {
  var allowed = SLOTS[agent.agent]
  var plans = agent.slot_plans
  if (!Array.isArray(plans) || plans.length > allowed.length) return false
  var seenSlots = {}, seenPlans = {}, lastFirst = -1
  for (var i = 0; i < plans.length; i++) {
    var plan = plans[i]
    if (!hasExactly(plan, ["plan_id", "plan_name", "trust_state", "trust_level", "threshold_millis", "slots"])) return false
    if (!isPlanId(plan.plan_id) || seenPlans[plan.plan_id] || !isLabel(plan.plan_name, 80) || !validTrust(plan)) return false
    seenPlans[plan.plan_id] = true
    if (!Array.isArray(plan.slots) || plan.slots.length === 0) return false
    var last = -1
    for (var j = 0; j < plan.slots.length; j++) {
      var index = allowed.indexOf(plan.slots[j])
      if (index <= last || seenSlots[plan.slots[j]]) return false
      seenSlots[plan.slots[j]] = true
      last = index
    }
    var first = allowed.indexOf(plan.slots[0])
    if (first <= lastFirst) return false
    lastFirst = first
  }
  return true
}

function validAgent(agent) {
  if (!hasExactly(agent, ["agent", "launchable", "check_reason_code", "slot_plans", "other_mapped_plans"])) return false
  if (!Object.prototype.hasOwnProperty.call(SLOTS, agent.agent) || typeof agent.launchable !== "boolean") return false
  if (agent.launchable ? agent.check_reason_code !== null : !isCode(agent.check_reason_code)) return false
  return isInt(agent.other_mapped_plans, 0, 4096) && validSlotPlans(agent)
}

function validSuggestions(value) {
  if (!hasExactly(value, ["count", "ids", "unavailable_plans"])) return false
  if (!isInt(value.count, 0, 4294967295) || !isInt(value.unavailable_plans, 0, 4096) || !Array.isArray(value.ids)) return false
  if (value.ids.length !== Math.min(value.count, MAX_IDS)) return false
  var seen = {}
  for (var i = 0; i < value.ids.length; i++) {
    if (!isSuggestionId(value.ids[i]) || seen[value.ids[i]]) return false
    seen[value.ids[i]] = true
  }
  return true
}

function validHeadline(value) {
  if (!isObject(value) || HEADLINES.indexOf(value.kind) < 0) return false
  var big = Number.MAX_SAFE_INTEGER
  switch (value.kind) {
  case "subscription_usage_unknown":
    return hasExactly(value, ["kind", "unknown_usage_requests"]) && isInt(value.unknown_usage_requests, 0, big)
  case "share":
    return hasExactly(value, ["kind", "outside_subscription_ratio_millis", "known_classified_tokens", "unknown_usage_requests"])
      && isInt(value.outside_subscription_ratio_millis, 0, 1000)
      && isInt(value.known_classified_tokens, 0, big) && isInt(value.unknown_usage_requests, 0, big)
  default:
    return hasExactly(value, ["kind"])
  }
}

function validSession(value, generatedAt, agents) {
  if (value === null) return true
  if (!hasExactly(value, ["session_id", "agent", "plan_id", "last_activity_ms"])) return false
  var known = false
  for (var i = 0; i < agents.length; i++) if (agents[i].agent === value.agent) known = true
  return known && isSessionId(value.session_id) && isPlanId(value.plan_id)
    && isInt(value.last_activity_ms, generatedAt - WINDOW_MS, generatedAt)
}

function validSummary(data) {
  if (!hasExactly(data, ["schema", "generated_at_ms", "service", "agents", "suggestions", "latest_routed_session", "token_share_today", "notifications"])) return false
  if (data.schema !== SCHEMA || !isInt(data.generated_at_ms, 1, Number.MAX_SAFE_INTEGER) || typeof data.notifications !== "boolean") return false
  if (!hasExactly(data.service, ["state", "reason_code"]) || !Array.isArray(data.agents)) return false
  if (data.service.state === "unavailable") {
    return isCode(data.service.reason_code) && data.agents.length === 0 && data.suggestions === null
      && data.latest_routed_session === null && data.token_share_today === null
  }
  if (data.service.state !== "ready" || data.service.reason_code !== null || data.agents.length > 2) return false
  var seen = {}
  for (var i = 0; i < data.agents.length; i++) {
    if (!validAgent(data.agents[i]) || seen[data.agents[i].agent]) return false
    seen[data.agents[i].agent] = true
  }
  return validSuggestions(data.suggestions) && validHeadline(data.token_share_today)
    && validSession(data.latest_routed_session, data.generated_at_ms, data.agents)
}

// Returns the validated summary, or null when the output is not one current summary.
function parse(output) {
  if (typeof output !== "string" || utf8Bytes(output) > MAX_BYTES) return null
  var envelope
  try {
    envelope = JSON.parse(output)
  } catch (e) {
    return null
  }
  if (!isObject(envelope) || envelope.status !== "succeeded" || !isObject(envelope.schema_version)
      || envelope.schema_version.major !== 2 || !validSummary(envelope.data)) return null
  return envelope.data
}

// Bar presentation. Text is chosen by key so the panel stays translatable.
// `percent` is -1 unless the headline is a share; it keeps at most one decimal.
function bar(summary) {
  if (!summary || summary.service.state !== "ready")
    return { percent: -1, decimals: 0, tooltipKey: "bar.unavailable", unknown: false, warning: true, agentWarning: false, dot: false }
  var headline = summary.token_share_today
  var agentWarning = false
  for (var i = 0; i < summary.agents.length; i++) if (!summary.agents[i].launchable) agentWarning = true
  var share = headline.kind === "share"
  var millis = share ? headline.outside_subscription_ratio_millis : 0
  return {
    percent: share ? millis / 10 : -1,
    decimals: share && millis % 10 !== 0 ? 1 : 0,
    tooltipKey: share ? "bar.share" : "headline." + headline.kind,
    unknown: headline.unknown_usage_requests > 0,
    warning: agentWarning,
    agentWarning: agentWarning,
    dot: summary.suggestions.count > 0
  }
}

// The trust presentation for one slot Plan: a catalog key plus the real threshold.
function trust(plan) {
  if (plan.trust_state === "uniform")
    return { key: "trust.level." + plan.trust_level, threshold: plan.threshold_millis }
  return { key: "trust." + plan.trust_state, threshold: -1 }
}

function slotKeys(plan) {
  var keys = []
  for (var i = 0; i < plan.slots.length; i++) keys.push("slot." + plan.slots[i])
  return keys
}
