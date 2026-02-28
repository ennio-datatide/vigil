#!/usr/bin/env node
import { Command } from 'commander';
import { auth } from './commands/auth.js';
import { cleanup } from './commands/cleanup.js';
import { down } from './commands/down.js';
import { ls } from './commands/ls.js';
import { start } from './commands/start.js';
import { status } from './commands/status.js';
import { up } from './commands/up.js';

const program = new Command();

program.name('praefectus').description('Praefectus — AI agent orchestration').version('0.1.0');

program
  .command('up')
  .description('Start the server and dashboard')
  .option('--daemon', 'Run in background')
  .action(up);

program.command('down').description('Stop the server and dashboard').action(down);

program
  .command('start')
  .description('Start a new agent session')
  .argument('<project>', 'Project directory path')
  .argument('<prompt>', 'Task prompt for the agent')
  .option('--skill <skill>', 'Skill to invoke')
  .option('--role <role>', 'Agent role (implementer, reviewer, fixer, custom)')
  .action(start);

program
  .command('ls')
  .description('List active sessions')
  .option('--all', 'Include completed sessions')
  .action(ls);

program
  .command('auth')
  .description('Check or manage authentication')
  .argument('[action]', 'Action: status, claude, codex', 'status')
  .action(auth);

program.command('status').description('Show server status').action(status);

program
  .command('cleanup')
  .description('Remove old worktrees from completed sessions')
  .action(cleanup);

program.parse();

export { program };
