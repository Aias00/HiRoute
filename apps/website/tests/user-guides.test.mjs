import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { guidePages, guideRoute, renderUserGuide } from '../src/lib/user-guides.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('every public user guide renders in Chinese and English', async () => {
  for (const slug of Object.keys(guidePages)) {
    for (const language of ['zh', 'en']) {
      const html = await renderUserGuide(slug, language);
      assert.match(html, /<h1[^>]*>/, `${slug}.${language} needs a title`);
      assert.match(html, /<h2[^>]*>/, `${slug}.${language} needs actionable sections`);
    }
  }
  assert.equal(guideRoute('quickstart', 'zh'), '/docs/');
  assert.equal(guideRoute('cli', 'en'), '/en/docs/cli/');
});

test('guides cover Desktop, Linux headless, and the released management CLI', async () => {
  const installZh = await renderUserGuide('install-macos', 'zh');
  const installEn = await renderUserGuide('install-macos', 'en');
  const linuxZh = await renderUserGuide('install-linux', 'zh');
  const linuxEn = await renderUserGuide('install-linux', 'en');
  const routing = await renderUserGuide('model-routing', 'zh');
  const worker = await renderUserGuide('cli', 'en');
  assert.match(installZh, /自签名/);
  assert.match(installZh, /仍要打开/);
  assert.match(installEn, /Open Anyway/);
  assert.match(linuxZh, /curl -fsSL https:\/\/hiroute\.ai\/install\.sh \| sh/);
  assert.match(linuxEn, /hiroute service start --output json/);
  assert.match(linuxEn, /hiroute service run/);
  assert.match(linuxEn, /hiroute compute scan --output json/);
  assert.match(linuxEn, /compute connection options\/test\/preview\/apply/);
  assert.match(linuxEn, /<code>authorize<\/code>/);
  assert.match(routing, /固定模型/);
  assert.match(routing, /按难度分流/);
  assert.match(routing, /自定义分支/);
  assert.match(routing, /免费优先/);
  assert.match(worker, /hiroute worker exec/);
  assert.match(worker, /--submission my-check-001 --operation start/);
  assert.match(worker, /routing options\/list\/show\/preview\/apply/);
  assert.match(worker, /sessions list/);

  const sources = await Promise.all(Object.keys(guidePages).flatMap(slug => ['zh', 'en'].map(language =>
    fs.readFile(path.join(root, 'content/guides', `${slug}.${language}.md`), 'utf8'))));
  const all = sources.join('\n');
  assert.match(all, /standalone/i);
  assert.doesNotMatch(all, /commands for creating model sources, editing routes, or connecting agents are available yet/i);
  assert.doesNotMatch(all, /这些配置入口仍由 Desktop 提供/);
});

test('decision guidance exposes current routing choices and released CLI paths in both languages', async () => {
  const manifest = JSON.parse(await fs.readFile(path.join(root, '../../contracts/cli/manifest.v1.json'), 'utf8'));
  const commandIds = ['decision.services.list', 'decision.services.apply', 'decision.services.test', 'observation.plan-quality.samples'];
  for (const language of ['zh', 'en']) {
    const routing = await renderUserGuide('model-routing', language);
    const modes = language === 'zh'
      ? ['固定模型', '按难度分流', '自定义分支', '免费优先']
      : ['Fixed model', 'Difficulty routing', 'Custom branches', 'Free first'];
    for (const mode of modes) assert.ok(routing.includes(`<td>${mode}</td>`), `${language} routing table is missing ${mode}`);

    const cli = await renderUserGuide('cli', language);
    for (const commandId of commandIds) {
      const command = manifest.commands.find(entry => entry.command_id === commandId);
      assert.equal(command?.lifecycle, 'released', `${commandId} must be public`);
      assert.ok(cli.includes(`hiroute ${command.path.join(' ')}`), `${language} CLI is missing ${commandId}`);
    }
    assert.doesNotMatch(cli, /(?:publishes|公开)\s*\d+\s*(?:Released|个)/);
  }
});

test('the Omarchy guide uses the installed integration help and states the account boundary', async () => {
  const source = await fs.readFile(path.join(root, '../../crates/cli/src/host_commands/omarchy.rs'), 'utf8');
  const help = JSON.parse(`"${source.match(/const HELP: &str = "((?:[^"\\]|\\.)*)";/)[1]}"`);
  const usage = help.split('\n').filter(line => line.startsWith('  hiroute integrations omarchy '));
  const manifest = JSON.parse(await fs.readFile(path.join(root, '../../contracts/cli/manifest.v1.json'), 'utf8'));
  for (const language of ['zh', 'en']) {
    const markdown = await fs.readFile(path.join(root, 'content/guides', `omarchy.${language}.md`), 'utf8');
    const commands = [...markdown.matchAll(/hiroute integrations omarchy ([a-z-]+)((?: --?[a-z-]+(?: [a-z-]+)?)*)/g)];
    assert.ok(commands.length >= 5, `${language} guide should show the integration commands`);
    for (const [, verb, options] of commands) {
      const line = usage.find(entry => entry.startsWith(`  hiroute integrations omarchy ${verb}`));
      assert.ok(line, `${language} guide uses unknown verb ${verb}`);
      // Arguments after the exact -- delimiter belong to the native Agent.
      for (const flag of `${options} `.split(' -- ')[0].match(/--[a-z-]+/g) ?? []) assert.ok(line.includes(flag), `${verb} has no ${flag}`);
    }
    for (const commandId of ['agent.launch', 'sessions.timeline']) {
      const command = manifest.commands.find(entry => entry.command_id === commandId);
      assert.equal(command?.lifecycle, 'released');
      assert.ok(markdown.includes(`hiroute ${command.path.join(' ')}`), `${language} guide is missing ${commandId}`);
    }
    const linux = await renderUserGuide('install-linux', language);
    assert.ok(linux.includes(`href="${guideRoute('omarchy', language)}"`), `${language} Linux guide must link the Omarchy guide`);
  }
  assert.match(await renderUserGuide('omarchy', 'en'), /They do not follow Omarchy&#39;s active account|They do not follow Omarchy's active account/);
  assert.match(await renderUserGuide('omarchy', 'zh'), /不跟随 Omarchy 当前账户/);
});
