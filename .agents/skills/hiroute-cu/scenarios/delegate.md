# DELEGATE: configure and run Worker tasks

Use Plan, Worker dependencies and a test directory. Main Agent can retain its
original connection; delegation does not require enabling its model routing.
Contract: `issue-spec/specs/agent-delegation/spec.md`. Minimum smoke verifies/saves
UI configuration as selected, a real main Agent delegates, Worker creates a real
file/test result and Desktop matches the task. Extend sessions/lifecycle with
Continue/cancel/failure, executors/clients with applicable pairs and publication
with active-task version behavior. No batch business script. Observe unknown
state before submitting again.

1. Configure delegation/executor and detect/save dependencies in Plan; configure
   main-agent Skill/trigger in Agent.
2. Actual main Agent explicitly delegates with an available purpose-selected plan.
   Check default delegation separately; prompts are not enforced interception and
   coordination actions must not recursively delegate.
3. Select Codex→Claude/Claude→Codex and relevant same-client pairs. Worker writes
   synthetic files/runs tests; main-agent result and Desktop agree.
4. Immediately Continue with files/marker/tool history in the same task/native
   session, not a disguised new task.
5. Check selected cancellation, timeout/failure, two tasks and reopened details;
   active publish/disable follows frozen-version and new-admission contracts.

Disconnection does not authorize resubmission. Read task/run on uncertainty, keep
missing Worker separate from model-routing availability, cancel only owned tasks
and restore owned configuration. App exit is not task completion. Report real
identity, frozen Plan, actual model/artifacts and current result in the Issue/PR;
a spawned/cleanly exited process alone does not prove success.
