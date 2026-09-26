import type { QuickLink } from "$lib/types/nav";
import type { Pathname } from "$app/types";
import {
    type Icon as IconType,

} from 'lucide-svelte';

export interface Feature {
    icon: typeof IconType;
    title: string;
    description: string;
}

/** A top-level section of the docs: a directory of `src/content`. */
export interface Section {
    /** The directory in `src/content`. */
    dir: string;

    /** The section's title in the navigation. */
    title: string;
}

export interface SiteConfig {
    /** The latest release of `crate` on crates.io, when the site was built */
    version: string;

    /** Main title of the documentation site */
    title: string;

    /** Detailed description of the project/documentation */
    description: string;

    /** GitHub repository URL */
    github: string;

    /** The crate whose latest release the site shows and links to */
    crate: string;

    /** The command that adds the project to a Cargo package */
    install: string;

    /** Array of quick navigation links, as site paths (without the base path) */
    quickLinks: QuickLink[];

    /** The sections of the docs, in navigation order; every directory of `src/content` is one */
    sections: Section[];

    /** Path to the main logo (light theme), in `static` */
    logo: string;

    /** Path to the dark theme logo, in `static` */
    logoDark: string;
}
export interface PromoConfig {
    title: string;
    description: string;
    ctaText: string;
    ctaLink: Pathname;
}
