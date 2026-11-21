import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// `pnpm dev` serves the UI itself and passes the API through to a control
// plane: by default the one `make run-local` starts.
const api = process.env.QARAX_URL ?? 'http://localhost:8000';

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// qarax-server serves the build at /ui (qarax/src/handlers/ui.rs).
			paths: { base: '/ui', relative: false },
			// A single-page app: index.html loads every page, and qarax-server
			// serves it for any path under /ui that isn't a file.
			adapter: adapter({ fallback: 'index.html', precompress: true })
		})
	],
	server: {
		proxy: { '/v1': { target: api, ws: true } }
	}
});
