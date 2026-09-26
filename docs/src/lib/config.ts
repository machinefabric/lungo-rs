
import type { NavItem, SocialLink } from "$lib/types/nav";

import {
    Cpu,
    FileCode2,
    ShieldCheck,
    Wrench
} from 'lucide-svelte';
import type { Feature, PromoConfig, SiteConfig } from "./types/config";


export const siteConfig: SiteConfig = {
    version: __LUNGO_VERSION__,
    title: 'lungo',
    description:
        'Compile ordinary Lake projects into Rust at Cargo build time. Lean checks and compiles the program; lungo runs it on a Rust port of Lean\'s runtime behind an idiomatic Rust API.',
    github: 'https://github.com/jowharshamshiri/lungo',
    crate: __LUNGO_CRATE__,
    install: 'cargo add lungo && cargo add --build lungo-build',

    quickLinks: [
        { title: 'First crate', href: '/docs/tutorials/first-crate' },
        { title: 'Configuration', href: '/docs/reference/configuration' },
        { title: 'Type mapping', href: '/docs/reference/type-mapping' },
        { title: 'Errors', href: '/docs/reference/errors' }
    ],
    sections: [
        { dir: 'tutorials', title: 'Tutorials' },
        { dir: 'how-to', title: 'How-to guides' },
        { dir: 'reference', title: 'Reference' },
        { dir: 'explanation', title: 'Explanation' }
    ],
    logo: '/logo.svg',
    logoDark: '/logo-white.svg',
};


export let navItems: NavItem[] = [
    {
        title: 'Docs',
        href: '/docs'
    },

];

export let socialLinks: SocialLink[] = [
    {
        title: 'GitHub',
        href: 'https://github.com/jowharshamshiri/lungo',
        icon: 'github'
    },
];


export const features: Feature[] = [
    {
        icon: Wrench,
        title: 'One line in build.rs',
        description: 'compile_lean("lean") builds the Lake project\'s default targets and generates a Rust module named after the package'
    },
    {
        icon: ShieldCheck,
        title: 'Checked by Lean',
        description: 'Lean elaborates and kernel-checks the project; a false proof or invalid Lean fails the Rust build'
    },
    {
        icon: Cpu,
        title: 'Pure Rust at run time',
        description: 'Compiled code runs on a Rust port of Lean\'s runtime: no Lean runtime or C code is linked'
    },
    {
        icon: FileCode2,
        title: 'Idiomatic Rust API',
        description: 'Lean structures become Rust structs, Nat an unbounded integer, IO a Result, with serde and your own attributes'
    }
];

export let promoConfig: PromoConfig = {
    title: 'New to lungo?',
    description:
        'Build a Rust crate from a few Lean definitions and a proof in about fifteen minutes.',
    ctaText: "Your first crate",
    ctaLink: '/docs/tutorials/first-crate'
};
