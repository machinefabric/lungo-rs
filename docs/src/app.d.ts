// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	/** The crate whose releases the site shows (vite.config.ts). */
	const __LUNGO_CRATE__: string;
	/** Its latest release on crates.io, when the site was built (vite.config.ts). */
	const __LUNGO_VERSION__: string;

	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}
}

export {};
