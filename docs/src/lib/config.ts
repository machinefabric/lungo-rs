
import type { NavItem, SocialLink } from "$lib/types/nav";

import {
    Cpu,
    FileCode2,
    Languages,
    ShieldCheck,
    Wrench
} from 'lucide-svelte';
import type { Feature, PromoConfig, SiteConfig } from "./types/config";


export const siteConfig: SiteConfig = {
    version: __LUNGO_VERSION__,
    title: 'lungo',
    description:
        'Generate Rust, C, Go, Python, Swift and TypeScript from ordinary Lake projects. Lean checks and compiles the program; lungo runs it on one runtime behind an idiomatic API in each language.',
    github: 'https://github.com/jowharshamshiri/lungo',
    crate: __LUNGO_CRATE__,
    install: 'cargo add lungo && cargo add --build lungo-build',

    quickLinks: [
        { title: 'First crate', href: '/docs/tutorials/first-crate' },
        { title: 'The lungo command', href: '/docs/reference/lungo-cli' },
        { title: 'Generated packages', href: '/docs/reference/generated-packages' },
        { title: 'Configuration', href: '/docs/reference/configuration' },
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
        description: 'Lean elaborates and kernel-checks the project; a false proof or invalid Lean fails the build'
    },
    {
        icon: Cpu,
        title: 'No Lean runtime',
        description: 'Compiled code runs on a Rust port of Lean\'s runtime, built into Rust crates or prebuilt for every other language'
    },
    {
        icon: FileCode2,
        title: 'Idiomatic APIs',
        description: 'Lean structures become Rust structs, Go structs, Python dataclasses, Swift structs, TypeScript objects; Nat an unbounded integer; IO an error'
    },
    {
        icon: Languages,
        title: 'Every language, one runtime',
        description: 'lungo generate --go_out --python_out --swift_out --ts_out --c_out: packages that run on the prebuilt runtime, verified by SHA-256'
    }
];

export let promoConfig: PromoConfig = {
    title: 'New to lungo?',
    description:
        'Build a Rust crate from a few Lean definitions and a proof in about fifteen minutes.',
    ctaText: "Your first crate",
    ctaLink: '/docs/tutorials/first-crate'
};
