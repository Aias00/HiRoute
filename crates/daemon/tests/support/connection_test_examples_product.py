"""Run the documented OpenRouter recipes through the real CLI and daemon."""
import json
import os
import subprocess
import sys
from pathlib import Path

from publication_product import Product


def recipe(repository, filename, heading):
    document = (repository / 'docs' / filename).read_text()
    section = document.split(heading + '\n', 1)[1]
    return section.split('```sh\n', 1)[1].split('\n```', 1)[0]


def main(repository):
    product = Product(repository)
    try:
        product.start()
        before = product.public_cli('compute list')[1]['data']['sources']
        documents = [
            ('standalone-cli.md', '## Test a catalog API connection'),
            ('standalone-cli.zh-CN.md', '## 测试目录中的 API 连接'),
        ]
        recipes = [recipe(product.repo, *document) for document in documents]
        assert recipes[0] == recipes[1], 'translated recipes have diverged'
        # Deliberately invalid: a local candidate must not imply the key was accepted.
        token = 'invalid-openrouter-recipe-regression-key'
        product.secrets.add(token)
        key_file = product.root / 'private-key'
        key_file.write_text(token)
        key_file.chmod(0o600)
        for index, script in enumerate(recipes):
            temporary = product.root / ('recipe-' + str(index))
            temporary.mkdir(mode=0o700)
            environment = dict(product.env, TMPDIR=str(temporary),
                               PATH=str(product.bin) + os.pathsep + product.env['PATH'])
            script = script.replace("KEY_FILE='/absolute/private/openrouter-key'",
                                    'KEY_FILE=' + str(key_file))
            result = subprocess.run(['/bin/sh'], input=script.encode(), env=environment,
                                    cwd=product.project, capture_output=True, timeout=45)
            product.outputs.extend([result.stdout, result.stderr])
            assert result.returncode == 0, (result.returncode, result.stdout, result.stderr)
            directories = list(temporary.iterdir())
            assert len(directories) == 1, directories
            work = directories[0]
            envelope = json.loads((work / 'result.json').read_text())
            assert envelope['status'] == 'succeeded', envelope
            check = envelope['data']
            expected = {'reachability': 'not_run', 'authentication': 'unknown',
                        'directory': 'not_run', 'inference': 'not_run'}
            assert {name: check[name] for name in expected} == expected, check
            assert check['inventory_path'] is None and check['pages_read'] == 0, check
            assert check['candidate']['models'], check
            request = json.loads((work / 'registered-request.json').read_text())
            assert request['kind'] == 'registered', request
            assert request['request']['connection_option_id'] == 'openrouter.free.global.v1'
            assert request['request']['inference_model_id'] is None
            # Successful commands and catalog models are insufficient verification evidence.
            assert b'Remote connectivity and API key were not verified.' in result.stdout, result.stdout
            guidance = json.loads((work / 'contract.json').read_text())['data']['schemas']['example_guidance']
            for name, value in expected.items():
                assert name + '=' + value in guidance, (name, guidance)
                assert (name + '=' + value).encode() in result.stdout, result.stdout
            # The EXIT trap releases the input: replaying the same request must now fail.
            replay_code, replay = product.public_cli('compute connection test', request,
                                                     success=False)
            assert replay_code != 0 and replay['status'] != 'succeeded', replay
            assert product.public_cli('compute list')[1]['data']['sources'] == before
            for path in work.iterdir():
                product.outputs.append(path.read_bytes())
            print(json.dumps({'scenario': documents[index][0], 'state': 'green',
                              'remote_verification': 'not_run', 'protected_input': 'released'}),
                  flush=True)
    finally:
        product.close()


if __name__ == '__main__':
    main(Path(sys.argv[1]))
