import { siteConfig } from "$lib/config";
import type { DocResolver } from "$lib/types/docs";
import type { NavItem } from "$lib/types/nav";

/**
 * The docs navigation: the root page, then one group per section of `siteConfig.sections`, in
 * that order, each listing its pages by title. Hrefs are site paths without the base path.
 */
class DocsNavigation {
    private static instance: DocsNavigation;
    public docNav = $state<NavItem[]>([]);

    private constructor() { }

    public static getInstance(): DocsNavigation {
        if (!DocsNavigation.instance) {
            DocsNavigation.instance = new DocsNavigation();
        }
        return DocsNavigation.instance;
    }

    public async generateNavigation(): Promise<NavItem[]> {
        const modules = import.meta.glob(`/src/content/**/*.md`);
        const root: NavItem[] = [];
        const sections = new Map<string, NavItem[]>(siteConfig.sections.map((s) => [s.dir, []]));

        for (const [path, resolver] of Object.entries(modules)) {
            const doc = await (resolver as DocResolver)();
            const { title, disabled, external, label } = doc.metadata;
            const slug = path.replace(/^\/src\/content\//, '').replace(/\.md$/, '').replace(/(^|\/)index$/, '');
            const item: NavItem = { title, href: slug === '' ? '/docs' : `/docs/${slug}`, disabled, external, label };

            if (slug === '') {
                root.push(item);
                continue;
            }
            const dir = slug.split('/')[0];
            const pages = sections.get(dir);
            if (pages === undefined || !slug.includes('/')) {
                throw new Error(
                    `${path} is not in a section: pages are src/content/index.md or in a directory listed in siteConfig.sections`
                );
            }
            pages.push(item);
        }

        this.docNav = [
            ...root,
            ...siteConfig.sections.map((section) => ({
                title: section.title,
                items: (sections.get(section.dir) ?? []).sort((a, b) => a.title.localeCompare(b.title))
            }))
        ];
        return this.docNav;
    }
}

export const docsNavigation = DocsNavigation.getInstance();
