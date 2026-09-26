import { resolve } from '$app/paths';
import type { Pathname } from '$app/types';

/** The URL of a site path, under the site's base path; none for a navigation group. */
export function link(href: Pathname | undefined): string | undefined {
	return href === undefined ? undefined : resolve(href);
}
