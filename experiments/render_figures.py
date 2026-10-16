#!/usr/bin/env python3
"""Deterministic article figures from verified published results. Requires matplotlib."""
import json
from pathlib import Path
from html import escape
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from reproduce import ROOT, RESULTS, report


def render_handoffs(out, result):
    evidence = json.loads((ROOT / 'cases/unattended-engineering/results/2026-10-04/evidence.json').read_text())
    stages = []
    for request in evidence['requests']:
        model, = request['models']
        if not stages or stages[-1] != model:
            stages.append(model)
    if stages != ['qwen3.8-flash', 'gpt-6-astra'] * 2:
        raise ValueError('The recorded model sequence changed; review the handoff narrative.')
    changes = len(stages) - 1
    checks = result['unattended']['independent_assertions']['total_passed']
    first_upgrade, final_upgrade = [decision for decision in evidence['decisions'] if decision['reason'] == 'competence_guard']
    for lang in ['zh', 'en']:
        zh = lang == 'zh'
        title = f'一项长任务，{changes} 次自动模型接力' if zh else f'One long task. {changes} automatic model handoffs.'
        names = ['Qwen 起步', 'Astra 整理交接', 'Qwen 接续', 'Astra 完成交付'] if zh else ['Qwen starts', 'Astra summarizes', 'Qwen continues', 'Astra delivers']
        roles = ['阅读源码与测试', '完成上下文摘要', '继续研究与探查', '实现、测试与修复'] if zh else ['Read source and tests', 'Prepare context summary', 'Continue investigation', 'Implement, test and repair']
        details = ['经济模型优先起步', '', '摘要交接后按策略回切', ''] if zh else ['Economy-first policy', '', 'Summary complete · return', '']
        for index, decision in [(1, first_upgrade), (3, final_upgrade)]:
            details[index] = f"{decision['competence']} < {decision['competence_floor']}" + (' · 升级' if zh else ' · upgrade')
        summary = (f'{changes} 次自动切换 · 零中途人工提示 · {checks} 项独立验收通过' if zh else f'{changes} automatic model changes · No operator prompts during execution · {checks} passing checks')
        note = '31 分 36 秒：阶段反馈触发升级，交接后按策略回切，进展不足时再次升级。' if zh else '31m 36s: upgrade on stage feedback, return after handoff, upgrade again when progress remains insufficient.'
        svg = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1240 350" role="img">',
               f'<title>{escape(title)}</title><rect width="1240" height="350" rx="20" fill="#eef2fa"/>',
               '<g font-family="system-ui,sans-serif" fill="#192439">',
               f'<text x="32" y="48" font-size="26" font-weight="700">{escape(title)}</text>']
        for i, x in enumerate([32, 340, 648, 956]):
            svg.append(f'<rect x="{x}" y="85" width="252" height="142" rx="14" fill="white"/>')
            for y, size, weight, value in [(122, 20, 700, names[i]), (162, 17, 400, roles[i]), (198, 15, 400, details[i])]:
                svg.append(f'<text x="{x+16}" y="{y}" font-size="{size}" font-weight="{weight}">{escape(value)}</text>')
            if i < len(stages) - 1:
                svg.append(f'<path d="M {x+261} 151 h 38 m -10 -7 l 10 7 -10 7" fill="none" stroke="#586caa" stroke-width="3"/>')
        svg.extend([f'<text x="32" y="277" font-size="22" font-weight="700">{escape(summary)}</text>',
                    f'<text x="32" y="316" font-size="16" fill="#536079">{escape(note)}</text>', '</g></svg>'])
        (out / f'unattended-handoff-{lang}.svg').write_text(''.join(svg) + '\n')


def _svg_frame(title, subtitle, height):
    return [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1000 {height}" role="img">',
            f'<title>{escape(title)}</title><rect width="1000" height="{height}" rx="20" fill="#eef2fa"/>',
            '<g font-family="system-ui,sans-serif" fill="#192439">',
            f'<text x="32" y="46" font-size="24" font-weight="700">{escape(title)}</text>',
            f'<text x="32" y="76" font-size="15" fill="#536079">{escape(subtitle)}</text>']


def _svg_text(x, y, value, size=14, anchor='start', weight=400, fill='#192439'):
    return f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" font-weight="{weight}" text-anchor="{anchor}" fill="{fill}">{escape(value)}</text>'


def _write_svg(path, parts):
    path.write_text(''.join(parts + ['</g></svg>']) + '\n')


def render_cache_cost(out):
    """Cache-cost article figures, drawn from the same calculator that verifies the article's numbers."""
    import math
    from cache_cost_analysis import analyze
    data = analyze()
    scenario = data['new_user_turn_scenario']
    prefix, new_input, growth = (scenario[k] for k in ('old_reusable_prefix_tokens', 'new_uncached_input_tokens', 'uncached_tokens_added_per_continuation'))
    pair = next(p for p in data['pairs'] if p['pair'] == 3)  # the accepted pair with the smaller margin
    if not pair['both_deliveries_accepted']:
        raise ValueError('The conservative figure pair must meet the delivery standard in both arms.')
    deliveries = json.loads((RESULTS / 'deliveries.json').read_text())['rows']
    strong_lower = float(next(r for r in deliveries if r['id'] == 'formal-draft98-v1-p03-strong')['usd']['lower'])
    ideal, mixed = float(pair['astra_all_input_cached_hypothetical_usd']), float(pair['mixed_usd_upper'])
    mixed_astra = float(pair['mixed_astra_usd_upper'])
    mixed_cached, mixed_output = float(pair['mixed_all_input_cached_hypothetical_usd']), float(pair['mixed_output_usd'])
    saved = float(pair['observed_savings_percent_floor'])
    saved_cached = float(pair['mixed_all_input_cached_savings_percent_floor'])
    break_even = float(pair['astra_price_break_even_percent_of_frozen']) / 100
    for lang in ['zh', 'en']:
        zh = lang == 'zh'
        # 1. One cold switch spread across the turn.
        parts = _svg_frame('切换一次，损失会被后续调用摊薄' if zh else 'One switch, spread across the turn',
                           '示例：10 万 token 旧对话 + 新消息，切换后第一次请求完全没有缓存' if zh else
                           'Example: a 100,000-token conversation, a new message, and a fully cold first request', 500)
        x0, x1, y0, y1 = 96, 950, 110, 390
        px = lambda n: x0 + (n - 1) / 19 * (x1 - x0)
        py = lambda v: y1 - v / 100 * (y1 - y0)
        rows = []
        for n in range(1, 21):
            total = n * (prefix + new_input) + growth * n * (n - 1) // 2
            uncached = new_input + (n - 1) * growth
            rows.append((n, (total - uncached) / total * 100, (total - uncached - prefix) / total * 100))
        for v in (0, 25, 50, 75, 100):
            parts.append(f'<path d="M {x0} {py(v):.1f} H {x1}" stroke="#d3d9e8"/>')
            parts.append(_svg_text(x0 - 10, py(v) + 5, f'{v}%', 13, 'end', fill='#536079'))
        for n in (1, 5, 10, 14, 20):
            parts.append(_svg_text(px(n), y1 + 24, str(n), 13, 'middle', fill='#536079'))
        parts.append(_svg_text((x0 + x1) / 2, y1 + 46, '这一轮的调用次数' if zh else 'Calls in the turn', 14, 'middle', fill='#536079'))
        gap = ' '.join(f'{px(n):.1f},{py(a):.1f}' for n, a, _ in rows) + ' ' + ' '.join(f'{px(n):.1f},{py(b):.1f}' for n, _, b in reversed(rows))
        parts.append(f'<polygon points="{gap}" fill="#f4c7a5" opacity=".55"/>')
        for index, color in ((1, '#5564bd'), (2, '#d0652a')):
            line = ' '.join(f'{px(r[0]):.1f},{py(r[index]):.1f}' for r in rows)
            parts.append(f'<polyline points="{line}" fill="none" stroke="{color}" stroke-width="3"/>')
        for n, a, b in rows:
            if n in (1, 5, 14, 20):
                parts.append(f'<circle cx="{px(n):.1f}" cy="{py(b):.1f}" r="5" fill="#d0652a"/>')
                label = (f'差 {a - b:.1f} 个百分点' if zh else f'{a - b:.1f} points lower')
                dy = -12 if n == 1 else 22
                parts.append(_svg_text(px(n) + (12 if n < 20 else -12), py(b) + dy, label, 13, 'start' if n < 20 else 'end', 700, '#9c4317'))
        for index, (name, color) in enumerate([('不切换' if zh else 'No switch', '#5564bd'), ('切换一次' if zh else 'One switch', '#d0652a')]):
            lx = x1 - 250 + index * 130
            parts.append(f'<path d="M {lx} 96 h 24" stroke="{color}" stroke-width="3"/>')
            parts.append(_svg_text(lx + 32, 101, name, 14, weight=700, fill=color))
        parts.append(_svg_text(32, 484, '整轮命中率 = 命中缓存的输入 ÷ 全部输入。规律：一轮调用 n 次，切换一次最多让命中率下降 1/n。' if zh else
                               'Turn hit rate = cached input ÷ all input. Rule: with n calls, one switch lowers it by at most 1/n.', 13, fill='#536079'))
        _write_svg(out / f'cache-amortization-{lang}.svg', parts)

        # 2. The same mixed run with and without a perfect cache, next to all-Astra.
        points = round(saved_cached - saved)
        parts = _svg_frame(f'缓存全命中，也只多省约 {points} 个百分点' if zh else f'A perfect cache would add only about {points} points of savings',
                           'Astra × Qwen 实验第 3 组（两组达标运行中缓存影响较大的一组），美元/单' if zh else
                           'Astra × Qwen experiment, pair 3 (the accepted pair where caching matters more), USD per task', 330)
        x0, x1, scale = 380, 860, 2.2
        bx = lambda v: x0 + v / scale * (x1 - x0)
        bars = [('全用 Astra（实际记录）' if zh else 'All Astra (recorded)', strong_lower, '#5564bd', None),
                ('HiRoute 混合（实际记录，Qwen 零缓存命中）' if zh else 'HiRoute mixed (recorded, no Qwen cache hits)', mixed, '#078677', saved),
                ('HiRoute 混合（假设输入全部命中缓存）' if zh else 'HiRoute mixed, if every input hit the cache', mixed_cached, '#5fb8a9', saved_cached)]
        y = 110
        for label, value, color, percent in bars:
            parts.append(_svg_text(x0 - 12, y + 21, label, 14, 'end'))
            parts.append(f'<rect x="{x0}" y="{y + 4}" width="{bx(value) - x0:.1f}" height="26" rx="4" fill="{color}"/>')
            parts.append(_svg_text(bx(value) + 8, y + 22, f'${value:.3f}', 14, weight=700))
            if percent is not None:
                parts.append(_svg_text(bx(max(mixed, mixed_cached)) + 84, y + 22, (f'省 {percent:.0f}%' if zh else f'{percent:.0f}% saved'), 15, weight=700, fill='#056356'))
            y += 52
        parts.append(_svg_text(32, y + 30, f'缓存只给输入打折。即使输入全部命中，混合路由剩下的 ${mixed_cached:.3f} 里仍有 ${mixed_output:.3f} 是输出费用。' if zh else
                               f'Caching only discounts input. Even fully cached, ${mixed_output:.3f} of the remaining ${mixed_cached:.3f} is output.', 13, fill='#536079'))
        _write_svg(out / f'cache-impact-{lang}.svg', parts)

        # 3. Hypothetical Astra price cuts: all-Astra cost as a multiple of mixed routing.
        parts = _svg_frame('Astra 要降价约 95%，两种方案才打平' if zh else 'Astra would need a ~95% price cut to break even',
                           '假设 Astra 各项单价同比例下调，Qwen 价格、用量和交付结果不变；全用 Astra 仍按输入全命中缓存计算' if zh else
                           'Uniform Astra price cuts; Qwen prices, usage and outcomes fixed; all-Astra still has every input cached', 505)
        x0, x1, y0, y1 = 96, 930, 110, 400
        low = .02
        sx = lambda s: x0 + (math.log(s) - math.log(low)) / -math.log(low) * (x1 - x0)
        sy = lambda r: y1 - r / 12 * (y1 - y0)
        ratio = lambda s: ideal * s / (mixed_astra * s + mixed - mixed_astra)
        parts.append(f'<rect x="{x0}" y="{sy(1):.1f}" width="{x1 - x0}" height="{y1 - sy(1):.1f}" fill="#f6dccb" opacity=".7"/>')
        parts.append(_svg_text(x1 - 10, y1 - 7, '低于 1×：全用 Astra 更便宜' if zh else 'Below 1×: all Astra is cheaper', 13, 'end', fill='#9c4317'))
        for r in (0, 1, 3, 6, 9, 12):
            parts.append(f'<path d="M {x0} {sy(r):.1f} H {x1}" stroke="#d3d9e8"/>')
            if r:
                parts.append(_svg_text(x0 - 10, sy(r) + 5, f'{r}×', 13, 'end', fill='#536079'))
        for s in (.02, .05, .1, .2, .5, 1):
            parts.append(_svg_text(sx(s), y1 + 24, f'{s * 100:.0f}%', 13, 'middle', fill='#536079'))
        parts.append(_svg_text((x0 + x1) / 2, y1 + 50, 'Astra 价格（今天 = 100%，对数刻度）' if zh else 'Astra price (today = 100%, log scale)', 14, 'middle', fill='#536079'))
        parts.append(_svg_text(x0, 100, '全用 Astra 的花费 ÷ 混合路由的花费' if zh else 'All-Astra cost ÷ mixed-routing cost', 13, fill='#536079'))
        steps = 160
        curve = ' '.join(f'{sx(low ** (1 - i / steps)):.1f},{sy(ratio(low ** (1 - i / steps))):.1f}' for i in range(steps + 1))
        parts.append(f'<polyline points="{curve}" fill="none" stroke="#5564bd" stroke-width="3"/>')
        for s in (1, .5, .2, .1):
            parts.append(f'<circle cx="{sx(s):.1f}" cy="{sy(ratio(s)):.1f}" r="5" fill="#5564bd"/>')
            parts.append(_svg_text(sx(s), sy(ratio(s)) - 12, (f'{ratio(s):.1f} 倍' if zh else f'{ratio(s):.1f}×'), 14, 'middle', 700, '#3a4796'))
        parts.append(f'<circle cx="{sx(break_even):.1f}" cy="{sy(1):.1f}" r="6" fill="#d0652a"/>')
        parts.append(_svg_text(sx(break_even) - 12, sy(1) - 14, (f'打平：约今天的 {break_even * 100:.1f}%' if zh else f'Break-even: about {break_even * 100:.1f}% of today'), 14, 'end', 700, '#9c4317'))
        parts.append(_svg_text(32, 488, '第 3 组数据；混合方案取成本上限，其中 Qwen 输入全部按未命中缓存计费。' if zh else
                               'Pair 3; mixed routing uses its cost upper bound, with every Qwen input billed as uncached.', 13, fill='#536079'))
        _write_svg(out / f'cache-price-break-even-{lang}.svg', parts)


def render():
    result = report()
    out = ROOT.parent / 'news/assets'
    out.mkdir(parents=True, exist_ok=True)
    rows = json.loads((RESULTS/'deliveries.json').read_text())['rows']
    plt.rcParams.update({'svg.hashsalt':'hiroute-news-20261004','svg.fonttype':'none','font.family':'DejaVu Sans'})
    from decimal import Decimal
    accepted = [p['pair'] for p in result['pairs'] if p['accepted']]
    costs = {}
    for group in ['strong', 'mixed']:
        selected = [next(r for r in rows if r['id'] == f"formal-draft98-v1-p{number:02d}-{group}") for number in accepted]
        costs[group] = tuple(sum(Decimal(row['usd'][bound]) for row in selected) / len(selected) for bound in ['lower', 'upper'])
    ratio = costs['mixed'][1] / costs['strong'][0]
    for lang in ['zh', 'en']:
        # SVG text remains selectable. Browser font fallback renders the Chinese
        # labels without requiring a platform-specific font in the generator.
        fig, ax = plt.subplots(figsize=(10, 4.8))
        zh = lang == 'zh'
        labels = ['全 Astra', 'HiRoute 混合'] if zh else ['All Astra', 'HiRoute mixed']
        for n, group in enumerate(['strong', 'mixed']):
            lo, hi = costs[group]
            percent = 100 if group == 'strong' else float(ratio * 100)
            color = '#5564bd' if group == 'strong' else '#078677'
            ax.barh(n, percent, height=.46, color=color)
            ax.text(percent + 2, n, f"${lo:.3f}–{hi:.3f}", va='center', fontsize=12, fontweight='bold', color='#192439')
        ax.set_yticks([0, 1], labels); ax.invert_yaxis(); ax.set_xlim(0, 132)
        ax.set_xticks([0, 25, 50, 75, 100], ['0%', '25%', '50%', '75%', '100%'])
        ax.set_xlabel('整单 API 等价成本 · 全 Astra = 100%' if zh else 'Whole-task API-equivalent cost · All Astra = 100%')
        ax.spines[['top', 'right', 'left']].set_visible(False)
        ax.set_title('相同交付标准，成本降低超过 90%' if zh else 'Same delivery standard. Over 90% lower cost.', loc='left', pad=20, fontsize=19)
        ax.text(130, 1, f"−{(1-ratio)*100:.2f}%", va='center', ha='right', fontsize=23, fontweight='bold', color='#087464')
        note = ('两次双方均达标运行的平均成本，包含全部尝试与路由评估。\n保守比较：混合成本上界 ÷ Astra 成本下界；完整三次结果见实验记录。' if zh else 'Mean cost across two runs meeting the same gate, including all attempts and routing evaluation.\nConservative comparison: mixed upper bound ÷ Astra lower bound. All three runs remain in the record.')
        fig.text(.08, .035, note, fontsize=9, color='#566078')
        fig.tight_layout(rect=[0, .14, 1, 1])
        target = out / f'research-cost-{lang}.svg'
        fig.savefig(target, metadata={'Date': None})
        target.write_text('\n'.join(line.rstrip() for line in target.read_text().splitlines()) + '\n')
        plt.close(fig)
    render_handoffs(out, result)
    render_cache_cost(out)


if __name__=='__main__':render()
