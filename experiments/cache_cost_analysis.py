#!/usr/bin/env python3
"""Recalculate the cache-cost article from frozen evidence; no network/model calls."""
from decimal import Decimal, ROUND_FLOOR
import json
from statistics import median

from reproduce import ROOT, RESULTS, assess, digest, read, require, verify_manifest


def analyze():
    verified = verify_manifest()
    evidence = read(RESULTS / 'evidence.json')
    deliveries = read(RESULTS / 'deliveries.json')
    assessment = assess(deliveries)
    basis = evidence['frozen_cost_basis']
    fx = Decimal(basis['fx']['usd_per_eur']) / Decimal(basis['fx']['cny_per_eur'])
    million = Decimal(1_000_000)
    rates = {}
    for model, record in basis['rates_per_million_tokens'].items():
        require(record['currency'] in ('USD', 'CNY'), 'unsupported currency')
        multiplier = fx if record['currency'] == 'CNY' else Decimal(1)
        rates[model] = {}
        for kind in ('input', 'cache_read', 'cache_write', 'output'):
            low, high = map(Decimal, record[kind])
            require(low == high, 'article needs a fixed tariff, not a price interval')
            rates[model][kind] = low * multiplier

    examples = []
    for model, rate in rates.items():
        for prefix in (100_000, 10_000):
            examples.append({
                'model': model, 'lost_reusable_prefix_tokens': prefix,
                'ordinary_processing_premium_usd': Decimal(prefix) * (rate['input'] - rate['cache_read']) / million,
                'cache_write_premium_usd': Decimal(prefix) * (rate['cache_write'] - rate['cache_read']) / million,
                'hit_rate_loss_percentage_points_at_1m_input': Decimal(prefix) / million * 100,
            })

    primary = rates['gpt-6-astra']
    writing_path = ROOT / 'results/decision-routing-20261006/results.json'
    writing = read(writing_path)['writing']
    writing_turns = [s for r in writing['replicates'] for s in r['stages']]
    complete_counts = [s['request_count'] for s in writing_turns if s['native_completed']]
    partial_counts = [s['request_count'] for s in writing_turns if not s['native_completed']]
    require(len(writing_turns) == writing['registered_turns'], 'writing turn coverage changed')
    require(len(complete_counts) == writing['normal_native_turns'], 'writing completion counts disagree')
    httpx = read(ROOT / 'cases/unattended-engineering/results/2026-10-04/evidence.json')
    require(httpx['single_native_turn'] and httpx['native_event_counts']['turn.started'] == 1,
            'HTTPX must remain a single native user turn')
    segments = []
    for request in sorted(httpx['requests'], key=lambda r: r['start_ms']):
        require(request['decision_source'] in ('external_classifier', 'inherited'), 'unknown decision source')
        if request['decision_source'] == 'external_classifier':
            segments.append([])
        require(bool(segments), 'continuation without opening decision')
        segments[-1].append(request)
    last_segment = segments[-1]
    require(all(r['outcome'] == 'accepted' and r['models'] == ['gpt-6-astra'] for r in last_segment),
            'final HTTPX segment must contain only accepted Astra requests')
    # Illustrative append-only user turn, not a measured experiment trace.
    old_prefix, new_input, continuation_growth = 100_000, 1_000, 2_000
    new_turn_examples = []
    for calls in sorted({1, 5, 20, len(last_segment)}):
        total = calls * (old_prefix + new_input) + continuation_growth * calls * (calls - 1) // 2
        uncached = new_input + (calls - 1) * continuation_growth
        new_turn_examples.append({
            'calls': calls, 'total_input_tokens': total,
            'no_switch_hit_percent': Decimal(total - uncached) / total * 100,
            'first_call_cold_switch_hit_percent': Decimal(total - uncached - old_prefix) / total * 100,
            'loss_percentage_points': Decimal(old_prefix) / total * 100,
            'append_only_single_switch_bound_percentage_points': Decimal(100) / calls,
        })
    directions = []
    for source, destination in (('gpt-6-astra', 'qwen3.8-flash'), ('qwen3.8-flash', 'gpt-6-astra')):
        warm = old_prefix * rates[source]['cache_read'] / million
        cold = old_prefix * rates[destination]['cache_write'] / million
        directions.append({
            'source': source, 'destination': destination,
            'source_warm_prefix_usd': warm, 'destination_cold_prefix_usd': cold,
            'old_prefix_cost_change_usd': cold - warm,
        })
    pairs = []
    for pair in assessment['pairs']:
        prefix = f"formal-draft98-v1-p{pair['pair']:02d}"
        arms = {row['group']: row for row in deliveries['rows'] if row['id'].startswith(prefix + '-')}
        strong, mixed = arms['strong'], arms['mixed']
        attempts = [a for c in strong['components'] for a in c['accounting']['attempts']]
        threshold = basis['rates_per_million_tokens']['gpt-6-astra']['long_input_threshold']
        require(all(a['model'] == 'gpt-6-astra' and a['usage']['input_tokens'] <= threshold for a in attempts),
                'all-Astra standard-context assumptions changed')
        inputs = sum(a['usage']['input_tokens'] for a in attempts)
        outputs = sum(a['usage']['output_tokens'] for a in attempts)
        decisions = sum((Decimal(c['accounting']['jev_reported_usd']) for c in strong['components']), Decimal(0))
        ideal = (inputs * primary['cache_read'] + outputs * primary['output']) / million + decisions
        mixed_upper = Decimal(mixed['usd']['upper'])
        headroom = ideal - mixed_upper
        mixed_attempts = [a for c in mixed['components'] for a in c['accounting']['attempts']]
        require({a['model'] for a in mixed_attempts} == {'gpt-6-astra', 'qwen3.8-flash'}, 'mixed arm models changed')
        mixed_astra = sum((Decimal(a['cost']['usd_upper']) for a in mixed_attempts if a['model'] == 'gpt-6-astra'), Decimal(0))
        # Scale every Astra tariff by one factor; Qwen attempts and Jev decisions keep their recorded cost.
        astra_break_even_scale = (mixed_upper - mixed_astra - decisions) / (ideal - decisions - mixed_astra)
        pairs.append({
            'pair': pair['pair'], 'both_deliveries_accepted': pair['accepted'],
            'observed_savings_percent_floor': pair['conservative_savings_percent'],
            'mixed_usd_upper': mixed_upper, 'astra_input_tokens': inputs, 'astra_output_tokens': outputs,
            'astra_all_input_cached_hypothetical_usd': ideal,
            'hypothetical_savings_percent_floor': ((1 - mixed_upper / ideal) * 100).quantize(Decimal('.01'), rounding=ROUND_FLOOR),
            'additional_cost_break_even_usd': headroom,
            'extra_astra_cache_write_tokens_at_break_even': headroom * million / (primary['cache_write'] - primary['cache_read']),
            'mixed_astra_usd_upper': mixed_astra, 'strong_decisions_usd': decisions,
            'mixed_qwen_output_tokens': sum(a['usage']['output_tokens'] for a in mixed_attempts if a['model'] == 'qwen3.8-flash'),
            'mixed_qwen_cache_read_tokens': sum(a['usage']['cache_read_tokens'] or 0 for a in mixed_attempts if a['model'] == 'qwen3.8-flash'),
            'astra_price_break_even_percent_of_frozen': astra_break_even_scale * 100,
        })
    return {
        'basis': 'Historical API-equivalent prices checked 2026-10-03; fixed 2026-09-30 FX, not current billing.',
        'scope': 'Post-hoc sensitivity calculation with observed outputs fixed; not measured cache-hit rates or a new run.',
        'verified_frozen_files': verified,
        'usd_per_cny': fx, 'usd_per_million_tokens': rates,
        'prefix_examples': examples, 'pairs': pairs,
        'observed_call_counts': {
            'unit': 'business-model requests per native user turn; not individual tool invocations',
            'writing_source': str(writing_path.relative_to(ROOT)), 'writing_source_sha256': digest(writing_path),
            'writing_completed': {
                'turns': len(complete_counts), 'requests': sum(complete_counts),
                'min': min(complete_counts), 'median': median(complete_counts), 'max': max(complete_counts),
                'mean': Decimal(sum(complete_counts)) / len(complete_counts),
            },
            'writing_interrupted': {'turns': len(partial_counts), 'requests': sum(partial_counts)},
            'httpx': {
                'native_user_turns': httpx['native_event_counts']['turn.started'],
                'requests': len(httpx['requests']),
                'accepted': sum(r['outcome'] == 'accepted' for r in httpx['requests']),
                'cancelled': sum(r['outcome'] == 'cancelled' for r in httpx['requests']),
                'compactions': len(httpx['compactions']),
                'inherited_decisions': sum(r['decision_source'] == 'inherited' for r in httpx['requests']),
                'inherited_decisions_percent': Decimal(sum(r['decision_source'] == 'inherited' for r in httpx['requests'])) / len(httpx['requests']) * 100,
                'final_accepted_astra_segment_requests': len(last_segment),
            },
        },
        'new_user_turn_scenario': {
            'kind': 'conditional illustration; fixed input sequence, one cold switch, no expiration or compaction',
            'old_reusable_prefix_tokens': old_prefix, 'new_uncached_input_tokens': new_input,
            'uncached_tokens_added_per_continuation': continuation_growth,
            'hit_rate_examples': new_turn_examples, 'prefix_cost_by_direction': directions,
        },
        'astra_price_sensitivity': 'Per-pair break-even assumes a uniform Astra price change; Qwen prices, usage and outcomes held fixed.',
        'original_three_pair_gate_passed': assessment['original_three_pair_gate_passed'],
        'paid_model_calls': 0,
    }


if __name__ == '__main__':
    print(json.dumps(analyze(), ensure_ascii=False, indent=2, default=str))
