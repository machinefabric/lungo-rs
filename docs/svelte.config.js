import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { escapeSvelte, mdsvex } from 'mdsvex';
import rehypeSlug from 'rehype-slug';
import { createHighlighter } from 'shiki';
import { readdirSync } from 'node:fs';
import { dirname, join, posix, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Where the site is served from: `/lungo` on GitHub Pages, empty on a custom domain. */
const base = process.env.BASE_PATH ?? '';
if (base !== '' && (!base.startsWith('/') || base.endsWith('/'))) {
	throw new Error(`BASE_PATH must be empty or start, and not end, with '/': ${JSON.stringify(base)}`);
}

const content = join(dirname(fileURLToPath(import.meta.url)), 'src', 'content');

/** The site path of a content file (`how-to/x.md` → `/docs/how-to/x`, `index.md` → `/docs`). */
function pagePath(file) {
	const slug = file.replace(/\.md$/, '').replace(/(^|\/)index$/, '');
	return slug === '' ? '/docs' : `/docs/${slug}`;
}

/** Every content file, relative to `src/content` and `/`-separated. */
function contentFiles(dir = content) {
	return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) return contentFiles(path);
		return entry.name.endsWith('.md') ? [relative(content, path).split(sep).join('/')] : [];
	});
}

/**
 * Rewrites relative links between content files (`../reference/errors.md#lng0107`), which is
 * how the Markdown sources link to each other so that they also work on GitHub, to the pages
 * the site serves them at (`<base>/docs/reference/errors#lng0107`). A link to a content file
 * that does not exist fails the build.
 */
function contentLinks() {
	const files = new Set(contentFiles());
	return (tree, file) => {
		const from = posix.dirname(relative(content, file.filename).split(sep).join('/'));
		const visit = (node) => {
			if (node.type === 'link' && !/^([a-z][a-z0-9+.-]*:|\/|#)/i.test(node.url)) {
				const [path, fragment] = node.url.split('#', 2);
				if (path.endsWith('.md')) {
					const target = posix.normalize(posix.join(from, path));
					if (!files.has(target)) {
						throw new Error(`${file.filename}: link to ${node.url}, but ${target} is not a page`);
					}
					node.url = base + pagePath(target) + (fragment === undefined ? '' : `#${fragment}`);
				}
			}
			node.children?.forEach(visit);
		};
		visit(tree);
	};
}

/**
 * Requires every content file to declare, in its front matter, the `title` and `description`
 * the site shows in its header, navigation and search.
 */
function requireFrontMatter() {
	return (tree, file) => {
		if (!file.filename.startsWith(content)) return;
		for (const key of ['title', 'description']) {
			const value = file.data.fm?.[key];
			if (typeof value !== 'string' || value.trim() === '') {
				throw new Error(`${file.filename}: the front matter has no ${key}`);
			}
		}
	};
}

/**
 * Highlights code blocks at build time, in light and dark themes (the page's `dark` class
 * selects one; see `app.css`), so that pages are complete when prerendered. A code block
 * whose language is not loaded here fails the build.
 */
const shiki = await createHighlighter({
	themes: ['github-light', 'github-dark'],
	langs: ['json', 'lean', 'rust', 'sh', 'toml']
});

function highlight(code, lang) {
	if (!lang) throw new Error(`a code block has no language:\n${code}`);
	const html = shiki.codeToHtml(code, { lang, themes: { light: 'github-light', dark: 'github-dark' } });
	return `{@html \`${escapeSvelte(html)}\`}`;
}

/** @type {import('@sveltejs/kit').Config} */
const config = {
	extensions: ['.svelte', '.md', '.svx'],
	preprocess: [
		vitePreprocess({}),
		mdsvex({
			extensions: ['.md', '.svx'],
			highlight: { highlighter: highlight },
			remarkPlugins: [requireFrontMatter, contentLinks],
			rehypePlugins: [rehypeSlug]
		})
	],

	kit: {
		adapter: adapter({
			pages: 'build',
			assets: 'build',
			fallback: '404.html',
			precompress: false,
			strict: true
		}),
		paths: {
			base
		},
		prerender: {
			entries: ['/', ...contentFiles().map(pagePath)],
			handleHttpError: 'fail',
			handleMissingId: 'fail'
		}
	}
};

export default config;
