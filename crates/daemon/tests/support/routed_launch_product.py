"""Real CLI/daemon launch admission with an isolated argv recorder, never a live model."""
import errno
import json
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import sys
import time

from model_connections_product import NativeUpstream, NATIVE_TOKEN, MODEL, save_native_source, publish_plan_and_agent
from publication_product import Product


RECORDER = r'''
import os, signal, time
if os.environ.get('HIROUTE_LAUNCH_CAPTURE'):
    capture = os.environ['HIROUTE_LAUNCH_CAPTURE']
    if os.environ.get('LAUNCH_TEST_WAIT'):
        def received(number, _frame):
            with open(capture + '.signal', 'w') as output: output.write(str(number))
            raise SystemExit(128 + number)
        signal.signal(signal.SIGINT, received)
        signal.signal(signal.SIGTERM, received)
    with open(capture, 'w') as output:
        json.dump({'argv': sys.argv[1:], 'argv_hex': [os.fsencode(arg).hex() for arg in sys.argv[1:]],
                   'home': os.environ.get('CODEX_HOME'), 'cwd': os.getcwd(),
                   'inherited': os.environ.get('LAUNCH_TEST_INHERITED'),
                   'tty': [os.isatty(fd) for fd in (0, 1, 2)]}, output)
    if os.environ.get('LAUNCH_TEST_WAIT'):
        while True: time.sleep(.01)
    print('native-launch-stdout')
    print('native-launch-stderr', file=sys.stderr)
    raise SystemExit(23)
'''


def wait_for(predicate, timeout=10):
    deadline = time.monotonic() + timeout
    while not predicate():
        assert time.monotonic() < deadline, 'owned launch did not reach its bounded checkpoint'
        time.sleep(.02)


def invoke(product, arguments, env, input=None):
    result = subprocess.run([str(product.bin / 'hiroute'), *arguments], cwd=product.project,
                            env=env, input=input, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=20)
    product.outputs.extend((result.stdout, result.stderr))
    return result


def terminal(product, env, answer, expected):
    master, slave = pty.openpty()
    process = subprocess.Popen([str(product.bin / 'hiroute'), 'agent', 'launch', '--agent', 'codex',
                                '--native-fallback=prompt', '--', 'original-native-prompt'],
                               cwd=product.project, env=env, stdin=slave, stdout=slave,
                               stderr=slave, start_new_session=True)
    os.close(slave)
    transcript = b''
    try:
        deadline = time.monotonic() + 15
        answered = False
        while time.monotonic() < deadline:
            if select.select([master], [], [], .1)[0]:
                try:
                    data = os.read(master, 8192)
                except OSError as error:
                    if error.errno == errno.EIO: break
                    raise
                if not data: break
                transcript += data
                if not answered and b'[y/N]' in transcript:
                    os.write(master, answer)
                    answered = True
            if process.poll() is not None: break
        assert process.wait(timeout=2) == expected, 'terminal fallback exit differs'
        assert answered, 'fallback did not ask the terminal user'
        assert len(transcript) < 16384
        product.outputs.append(transcript)
        return transcript
    finally:
        os.close(master)
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=3)


def run(repository):
    product = Product(repository)
    upstream = NativeUpstream(base_host='localhost')
    try:
        codex = product.install_codex_fixture(MODEL, 'medium', upstream.base_url, NATIVE_TOKEN)
        original = codex.read_text()
        marker = 'provider = next('
        assert marker in original
        codex.write_text(original.replace(marker, RECORDER + '\n' + marker, 1))
        product.codex_access_mode = 'profile'
        product.start()
        capture = product.root / 'launch.json'
        env = dict(product.env, HIROUTE_LAUNCH_CAPTURE=str(capture),
                   LAUNCH_TEST_INHERITED='preserved-caller-environment')
        check_args = ['agent', 'launch', '--agent', 'codex', '--check']
        missing = invoke(product, check_args, env)
        assert missing.returncode == 78 and not capture.exists()
        assert json.loads(missing.stdout)['data']['launchable'] is False
        saved = save_native_source(product, upstream, NATIVE_TOKEN, cli_input=True)
        product.public_cli('protected-input release --candidate candidate/native/product')
        publish_plan_and_agent(product, saved['binding_id'], native_model_mode='hiroute_only')
        context = product.agent_context_id
        for arguments in (check_args, ['--output', 'json', *check_args],
                          [*check_args, '--context', context]):
            ready = invoke(product, arguments, env)
            assert ready.returncode == 0, ready.stderr
            data = json.loads(ready.stdout)['data']
            assert data['schema'] == 'hiroute.agent-launch-check/v1' and data['launchable']
            assert [item['check'] for item in data['checks']] == ['service', 'connection', 'plans']
            assert all(item['ok'] for item in data['checks']) and data['context'] == context
            assert not capture.exists(), '--check started an Agent session'
        tail = ['-m', product.model_alias, '-c', 'model_reasoning_effort="high"', 'opaque prompt']
        launched = invoke(product, ['agent', 'launch', '--agent', 'codex', '--', *tail], env)
        assert launched.returncode == 23
        assert b'native-launch-stdout' in launched.stdout and b'native-launch-stderr' in launched.stderr
        observed = json.loads(capture.read_text())
        assert observed['argv'] == ['--profile', 'hiroute', *tail]
        assert observed['home'] == str(Path(product.env['CODEX_HOME']).resolve())
        assert observed['cwd'] == str(product.project) and observed['inherited'] == env['LAUNCH_TEST_INHERITED']
        capture.unlink()
        # The CLI must not translate opaque native bytes through a UTF-8 argument parser.
        launched = invoke(product, ['agent', 'launch', '--agent', 'codex', '--', b'opaque-\xff'], env)
        assert launched.returncode == 23
        assert json.loads(capture.read_text())['argv_hex'][-1] == b'opaque-\xff'.hex()
        capture.unlink()
        for protected in (['--profile', 'other'], ['-hpother'], ['--oss'],
                          ['-hcmodel_provider="private-marker"'],
                          ['-c', 'profiles.hiroute.model="private-marker"']):
            denied = invoke(product, ['agent', 'launch', '--agent', 'codex', '--', *protected], env)
            assert denied.returncode == 2 and not capture.exists()
            assert b'private-marker' not in denied.stdout + denied.stderr
        for number in (signal.SIGINT, signal.SIGTERM):
            waiting = dict(env, LAUNCH_TEST_WAIT='1')
            process = subprocess.Popen([str(product.bin / 'hiroute'), 'agent', 'launch', '--agent',
                                        'codex', '--', 'wait-for-signal'], cwd=product.project,
                                       env=waiting, stdin=subprocess.DEVNULL,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       start_new_session=True)
            try:
                wait_for(capture.exists)
                # Ctrl-C is delivered to the shared foreground group by the terminal. The
                # existing supervisor forwards TERM/HUP addressed only to the launcher.
                if number == signal.SIGINT:
                    os.killpg(process.pid, number)
                else:
                    process.send_signal(number)
                stdout, stderr = process.communicate(timeout=10)
                product.outputs.extend((stdout, stderr))
                assert process.returncode == 128 + number
                assert int(Path(str(capture) + '.signal').read_text()) == number
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=3)
            capture.unlink()
            Path(str(capture) + '.signal').unlink()
        codex.write_text(codex.read_text() + '\n# changed selected installation\n')
        changed = invoke(product, check_args, env)
        assert changed.returncode == 78 and not capture.exists()
        assert 'installation_changed' in changed.stdout.decode()
        product.stop()
        stopped = invoke(product, check_args, env)
        assert stopped.returncode == 78 and not capture.exists()
        assert json.loads(stopped.stdout)['data']['checks'][0]['reason_code'] == 'service_unavailable'
        piped = invoke(product, ['agent', 'launch', '--agent', 'codex', '--native-fallback=prompt', '--'], env, b'y\n')
        assert piped.returncode == 78 and not capture.exists() and b'[y/N]' not in piped.stderr
        for answer in (b'n\n', b'Y\n', b'\n'):
            terminal(product, env, answer, 78)
            assert not capture.exists()
        transcript = terminal(product, env, b'y\n', 23)
        assert b'This session is not routed through HiRoute.' in transcript
        observed = json.loads(capture.read_text())
        assert observed['argv'] == ['original-native-prompt'] and all(observed['tty'])
        assert observed['home'] == env['CODEX_HOME'] and observed['inherited'] == env['LAUNCH_TEST_INHERITED']
        print(json.dumps({'scenario': 'routed-launch-real-cli-daemon', 'state': 'green',
                          'routed_sessions': 4, 'native_fallbacks': 1,
                          'terminal_refusals': 3, 'signal_cases': 2, 'live_model_calls': 0}), flush=True)
    finally:
        product.close()
        upstream.close()


if __name__ == '__main__':
    run(sys.argv[1])
