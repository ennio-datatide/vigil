import { buildApp } from './app.js';
import { loadConfigFile, resolveConfig } from './config.js';

async function main() {
  const config = resolveConfig();
  const fileConfig = loadConfigFile(config.configFile);
  const app = await buildApp(fileConfig);

  try {
    await app.listen({ port: config.serverPort, host: '0.0.0.0' });
    console.log(`Praefectus server running on http://0.0.0.0:${config.serverPort}`);
  } catch (err) {
    app.log.error(err);
    process.exit(1);
  }

  const shutdown = async () => {
    console.log('Shutting down...');
    await app.close(); // This triggers the onClose hook which disposes terminals and closes SQLite
    process.exit(0);
  };

  process.on('SIGTERM', shutdown);
  process.on('SIGINT', shutdown);
}

main();
