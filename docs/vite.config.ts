import { sveltekit } from '@sveltejs/kit/vite';
import { readFileSync } from 'node:fs';
import { defineConfig } from 'vite';

/** The version of the lungo crates: `version` in the workspace manifest's `[workspace.package]`. */
function lungoVersion(): string {
	const manifest = readFileSync(new URL('../Cargo.toml', import.meta.url), 'utf8');
	let inPackage = false;
	for (const line of manifest.split('\n')) {
		if (line.startsWith('[')) inPackage = line.trim() === '[workspace.package]';
		const version = inPackage && /^version = "(.*)"/.exec(line);
		if (version) return version[1];
	}
	throw new Error('../Cargo.toml has no [workspace.package] version');
}

export default defineConfig({
	plugins: [sveltekit()],
	define: {
		__LUNGO_VERSION__: JSON.stringify(lungoVersion())
	},
	build: {
		target: 'esnext'
	}
});
