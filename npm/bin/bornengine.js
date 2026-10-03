#!/usr/bin/env node
'use strict';

const { spawnSync } = require('node:child_process');
const { ensureBinary } = require('../lib/launcher.cjs');

async function main() {
  try {
    const binary = await ensureBinary();
    const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' });
    if (result.error) throw result.error;
    process.exitCode = result.status ?? 1;
  } catch (error) {
    console.error(`bornengine: ${error.message}`);
    process.exitCode = 1;
  }
}

main();
