const INSTALL_COMMAND = 'curl -fsSL https://tacoshells.dev/install.sh | sh';

const TERMINALS = [
  {
    kind: 'working',
    question: '',
    lines: [
      { text: '~/code/api ❯ claude', tone: 'dim' },
      { text: '', tone: 'ink' },
      { text: '> The login test fails about one run in five. Find out why.', tone: 'ink' },
      { text: '', tone: 'ink' },
      { text: 'Reading tests/test_login.py', tone: 'dim' },
      { text: 'Reading app/session.py', tone: 'dim' },
      { text: 'Running the test twenty times (run 7)', tone: 'ink' },
    ],
    log: [
      { time: '14:32', kind: 'Did', text: 'Started twenty runs of tests/test_login.py' },
      { time: '14:31', kind: 'Did', text: 'Read tests/test_login.py and app/session.py' },
      { time: '14:30', kind: 'Asked', text: 'The login test fails about one run in five. Find out why.' },
    ],
  },
  {
    kind: 'needs',
    question: 'Run the migration against the dev database?',
    lines: [
      { text: '> Add a nullable billing_email to accounts.', tone: 'ink' },
      { text: '', tone: 'ink' },
      { text: 'Reading models/account.py', tone: 'dim' },
      { text: 'Wrote migrations/0042_billing_email.py', tone: 'ink' },
      { text: '', tone: 'ink' },
      { text: 'Run the migration against the dev database?', tone: 'ink' },
    ],
    log: [
      { time: '14:28', kind: 'Changed', text: 'migrations/0042_billing_email.py' },
      { time: '14:27', kind: 'Did', text: 'Read models/account.py' },
      { time: '14:26', kind: 'Asked', text: 'Add a nullable billing_email to accounts.' },
    ],
  },
  {
    kind: 'done',
    question: '',
    lines: [
      { text: '> Bump the patch versions and run the tests.', tone: 'ink' },
      { text: '', tone: 'ink' },
      { text: 'Updated 6 packages', tone: 'ink' },
      { text: '212 passed in 41s', tone: 'green' },
      { text: '', tone: 'ink' },
      { text: 'Done. Nothing else needed changing.', tone: 'ink' },
    ],
    log: [
      { time: '14:19', kind: 'Did', text: 'Ran the tests: 212 passed' },
      { time: '14:17', kind: 'Changed', text: 'package.json and package-lock.json' },
      { time: '14:16', kind: 'Asked', text: 'Bump the patch versions and run the tests.' },
    ],
  },
  {
    kind: 'shell',
    question: '',
    lines: [
      { text: '~/code/api ❯ git status --short', tone: 'dim' },
      { text: ' M app/session.py', tone: 'ink' },
      { text: '', tone: 'ink' },
      { text: '~/code/api ❯', tone: 'dim' },
    ],
    log: [],
  },
];

const rows = document.querySelectorAll('.row');
const terminalPane = document.getElementById('terminal');
const logPane = document.getElementById('log');
const questionCard = document.getElementById('question');
const questionText = document.getElementById('question-text');
const noAgent = document.getElementById('no-agent');
const copyButton = document.getElementById('copy-button');
const copyCheck = document.getElementById('copy-check');
const copyLabel = document.getElementById('copy-label');

function makeSpan(className, text) {
  const span = document.createElement('span');
  span.className = className;
  span.textContent = text;
  return span;
}

function renderLines(lines) {
  terminalPane.replaceChildren();
  lines.forEach((line) => {
    const div = document.createElement('div');
    div.className = 'line line-' + line.tone;
    div.textContent = line.text;
    terminalPane.appendChild(div);
  });
}

function renderLog(log) {
  logPane.replaceChildren();
  log.forEach((item) => {
    const entry = document.createElement('div');
    entry.className = 'entry';
    const body = document.createElement('span');
    body.className = 'entry-body';
    body.appendChild(makeSpan('entry-kind', item.kind));
    body.appendChild(document.createTextNode(' '));
    body.appendChild(makeSpan('entry-text', item.text));
    entry.appendChild(makeSpan('entry-time', item.time));
    entry.appendChild(body);
    logPane.appendChild(entry);
  });
}

function select(index) {
  const terminal = TERMINALS[index];
  const hasAgent = terminal.kind !== 'shell';
  const hasQuestion = terminal.question !== '';

  rows.forEach((row, rowIndex) => {
    const isSelected = rowIndex === index;
    row.classList.toggle('is-selected', isSelected);
    row.setAttribute('aria-pressed', String(isSelected));
  });

  renderLines(terminal.lines);
  renderLog(terminal.log);
  questionText.textContent = terminal.question;
  questionCard.hidden = !hasQuestion;
  logPane.hidden = !hasAgent;
  noAgent.hidden = hasAgent;
}

function showCopied() {
  copyLabel.textContent = 'Copied';
  copyCheck.removeAttribute('hidden');
}

async function copyCommand() {
  try {
    await navigator.clipboard.writeText(INSTALL_COMMAND);
  } catch {
    return;
  }
  showCopied();
}

rows.forEach((row, rowIndex) => {
  row.addEventListener('click', () => select(rowIndex));
});

copyButton.addEventListener('click', copyCommand);
