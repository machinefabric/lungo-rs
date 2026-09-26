import { type Icon as IconType } from 'lucide-svelte';
import type { Pathname } from '$app/types';


export interface NavItem {
    title: string;
    /** The page's site path; none for a group of pages. */
    href?: Pathname;
    disabled?: boolean;
    external?: boolean;
    icon?: typeof IconType;
    label?: string;
    items?: NavItem[];
};

export interface SocialLink {
    title: string;
    href: string;
    icon: keyof Icons;
}

export interface Icons {
    twitter: string;
    github: string;
    facebook: string;
    instagram: string;
    linkedin: string;
    youtube: string;
    tiktok: string;
    snapchat: string;
}


export interface QuickLink {
    title: string;
    href: Pathname;
}