"use client";

import { useState } from "react";
import Image from "next/image";
import Link from "next/link";
import site from "@/content/site.json";

const { nav } = site;

export default function Header() {
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);

  const toggleMenu = () => setMobileMenuOpen(!mobileMenuOpen);
  const closeMenu = () => setMobileMenuOpen(false);

  return (
    <header className="fixed top-0 left-0 right-0 z-50">
      <div className="mx-4 lg:mx-8 mt-4">
        <nav className="glass-strong rounded-2xl px-6 lg:px-8">
          <div className="flex items-center justify-between h-16 lg:h-18">
            {/* Logo */}
            <Link href="/" className="flex items-center gap-2">
              <Image
                src="/logo.svg"
                alt="DChat Logo"
                width={40}
                height={40}
                className="rounded-xl"
              />
              <span className="text-xl font-bold text-white">{nav.logo}</span>
            </Link>

            {/* Desktop Navigation */}
            <div className="hidden md:flex items-center gap-1">
              {nav.links.map((link) => (
                <a
                  key={link.href}
                  href={link.href}
                  className="px-4 py-2 text-gray-400 hover:text-white transition-all text-sm font-medium rounded-lg hover:bg-white/5"
                >
                  {link.label}
                </a>
              ))}
            </div>

            {/* CTA Button */}
            <div className="hidden md:flex items-center gap-3">
              <a
                href="#"
                className="px-4 py-2 text-gray-400 hover:text-white transition-colors text-sm font-medium"
              >
                Sign In
              </a>
              <a
                href={nav.cta.href}
                className="btn-primary px-6 py-2.5 rounded-xl text-sm font-semibold text-white"
              >
                {nav.cta.label}
              </a>
            </div>

            {/* Mobile Menu Button */}
            <button
              onClick={toggleMenu}
              className="md:hidden p-2 text-gray-400 hover:text-white rounded-lg hover:bg-white/5"
              aria-label="Toggle menu"
            >
              <svg
                className="w-6 h-6"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                {mobileMenuOpen ? (
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M6 18L18 6M6 6l12 12"
                  />
                ) : (
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M4 6h16M4 12h16M4 18h16"
                  />
                )}
              </svg>
            </button>
          </div>

          {/* Mobile Navigation */}
          {mobileMenuOpen && (
            <div className="md:hidden pb-6">
              <div className="flex flex-col gap-2 pt-4 border-t border-white/10">
                {nav.links.map((link) => (
                  <a
                    key={link.href}
                    href={link.href}
                    onClick={closeMenu}
                    className="px-4 py-3 text-gray-400 hover:text-white transition-colors font-medium rounded-lg hover:bg-white/5"
                  >
                    {link.label}
                  </a>
                ))}
                <a
                  href={nav.cta.href}
                  onClick={closeMenu}
                  className="mt-2 btn-primary px-5 py-3 rounded-xl font-semibold text-center text-white"
                >
                  {nav.cta.label}
                </a>
              </div>
            </div>
          )}
        </nav>
      </div>
    </header>
  );
}
