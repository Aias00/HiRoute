import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { evaluate, navigate, waitFor } from './cdp-client.mjs';

function share(kind) {
  const bucket = (tokens, requests) => ({ tokens: { input: tokens, cache_read: 0, cache_write: 0, output: 0, total: tokens }, requests });
  const billing = { subscription: bucket(6_900_000, 2), paid: bucket(3_100_000, 2), free: bucket(0, 0), unknown: bucket(200, 1) };
  let headline = { kind };
  let unknown = 0;
  if (kind === 'share') { headline = { kind, outside_subscription_ratio_millis: 310, known_classified_tokens: 10_000_000, unknown_usage_requests: 2 }; unknown = 2; }
  if (kind === 'no_routed_requests') for (const key of Object.keys(billing)) billing[key] = bucket(0, 0);
  if (kind === 'no_subscription_in_period') { billing.subscription = bucket(0, 0); billing.unknown = bucket(0, 0); }
  if (kind === 'subscription_usage_unknown') { billing.subscription = bucket(0, 1); unknown = 1; headline = { kind, unknown_usage_requests: unknown }; }
  if (kind === 'no_comparable_usage') { billing.subscription = bucket(0, 0); billing.paid = bucket(0, 0); }
  return { basis: 'input_uncached+cache_read+cache_write+output', period: { kind: 'today', timezone: 'local (UTC+08:00)', start_ms: 1792425600000, end_ms: 1792468800000 },
    agent_filter: null, by_billing_class: billing, by_group: { regular: { tokens: billing.paid.tokens, turns: billing.paid.requests }, primary: { tokens: billing.subscription.tokens, turns: billing.subscription.requests }, other: { tokens: billing.unknown.tokens, turns: billing.unknown.requests } },
    unknown_usage_requests: unknown, retention_boundary_partial: kind === 'no_comparable_usage', headline, disclosure: 'Approximate across models; not quota, cost or savings.' };
}

export async function runTokenShareCaptures(client, fixtureUrl, outputRoot) {
  const checks = [];
  for (const language of ['zh', 'en']) for (const width of [755, 1200]) for (const theme of ['light', 'dark']) {
    const viewport = { width, height: width === 755 ? 600 : 813 };
    const t = (zh, en) => language === 'zh' ? zh : en;
    await navigate(client, fixtureUrl('product-shell.html'), viewport);
    await waitFor(client, 'Boolean(window.productShell)');
    for (const kind of ['share', 'no_routed_requests', 'no_subscription_in_period', 'subscription_usage_unknown', 'no_comparable_usage', 'loading', 'error']) {
      await evaluate(client, `(() => {const c=productShell; c.reset();localStorage.setItem('hiroute.language',${JSON.stringify(language)});localStorage.setItem('hiroute.theme',${JSON.stringify(theme)});
        window.tokenShareState={kind:${JSON.stringify(kind)},share:${JSON.stringify(share(kind))},calls:0};
        c.handlers.observation_read=p=>{if(p.request.intent.view==='home_value' && p.request.intent.query.breakdown==='billing_class') {
          const s=tokenShareState;s.calls++; if(p.request.intent.query.period!=='today')throw Error('Wrong period');
          if(s.kind==='loading')return new Promise(()=>{});if(s.kind==='error')throw Error('READ_UNAVAILABLE');return {...c.fixtureResponse('observation_read',p),token_share:s.share};
        } return c.fixtureResponse('observation_read',p);};c.remount();return true;})()`);
      const labels = { share: '31%', no_routed_requests: t('此时段没有路由请求', 'No routed requests in this period'), no_subscription_in_period: t('此时段未使用订阅来源', 'No subscription source used in this period'),
        subscription_usage_unknown: t('订阅来源的用量未知', 'Subscription usage is unknown'), no_comparable_usage: t('没有可比较的用量', 'No comparable usage'), loading: t('正在读取', 'Reading'), error: t('暂时无法读取', 'Temporarily unavailable') };
      await waitFor(client, 'tokenShareState.calls === 1');
      await waitFor(client, `document.querySelector('[data-token-share-headline]')?.textContent.includes(${JSON.stringify(labels[kind])})`);
      assert.equal(await evaluate(client, 'tokenShareState.calls'), 1);
      if (kind !== 'share') assert.equal(await evaluate(client, 'document.querySelector("[data-token-share-headline]").textContent.includes("%")'), false);
      if (kind === 'share') {
        assert.ok((await evaluate(client, 'document.querySelector("[data-token-share-unknown]").textContent')).includes(t('部分用量未知', 'Some usage unknown')));
        await evaluate(client, 'document.querySelector(".token-share-explanation button").click();true');
        await waitFor(client, '!!document.querySelector("[role=tooltip]")');
        const tooltip = await evaluate(client, 'document.querySelector("[role=tooltip]").textContent');
        assert.ok(tooltip.includes(t('缓存读取', 'cache reads')) && tooltip.includes('UTC+08:00'));
        assert.ok(tooltip.includes(t('不代表额度、费用或节省', 'not quota, cost or savings')));
      }
      const geometry = await evaluate(client, `Array.from(document.querySelectorAll('[data-token-share], [role=tooltip]')).map(e=>{const r=e.getBoundingClientRect();return {width:e.clientWidth,scroll:e.scrollWidth,left:r.left,right:r.right,top:r.top,bottom:r.bottom};})`);
      for (const r of geometry) { assert.ok(r.scroll <= r.width + 1, JSON.stringify(r)); assert.ok(r.left >= -1 && r.right <= width + 1, JSON.stringify(r)); }
      const file = `token-share-${kind}-${language}-${width}-${theme}.png`;
      const shot = await client.send('Page.captureScreenshot', { format: 'png' });
      await writeFile(path.join(outputRoot, file), Buffer.from(shot.data, 'base64'));
      if (kind === 'error') {
        await evaluate(client, `tokenShareState.kind='share';tokenShareState.share=${JSON.stringify(share('share'))};document.querySelector('[data-token-share] > button').click();true`);
        await waitFor(client, 'document.querySelector("[data-token-share-headline]")?.textContent.includes("31%")');
        assert.equal(await evaluate(client, 'tokenShareState.calls'), 2);
      }
      assert.equal(await evaluate(client, 'productShell.commands.filter(c=>["preview_agent_settings","check_agent_live","preview_plan_editor"].includes(c.command)).length'), 0);
      const id = `home.token-share.${kind}.${language}.${width}.${theme}`;
      checks.push({ id, name: id, state: 'green', language, viewport, theme, textScale: 1, screenshot: file });
    }
  }
  assert.equal(checks.length, 56, 'Required state/language/window/theme combinations were not executed');
  return checks;
}
