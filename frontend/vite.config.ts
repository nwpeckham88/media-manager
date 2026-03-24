import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
	const env = loadEnv(mode, '.', '');

	return {
		plugins: [sveltekit()],
		server: {
			proxy: {
				'/api': {
					target: env.MM_API_TARGET ?? 'http://127.0.0.1:8080',
					changeOrigin: true
				}
			}
		}
	};
});
