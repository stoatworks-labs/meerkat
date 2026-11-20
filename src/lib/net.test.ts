import { describe, expect, it } from 'vitest';

import { inSubnet, nth, parsePrefix, prefixToMask, problems, suggest } from './net';

describe('net helpers', () => {
  it('reads a mask the way people type it', () => {
    expect(parsePrefix('24')).toBe(24);
    expect(parsePrefix('/16')).toBe(16);
    expect(parsePrefix('255.255.255.0')).toBe(24);
    expect(parsePrefix('255.255.252.0')).toBe(22);
    expect(parsePrefix('255.0.255.0')).toBeNull();
    expect(parsePrefix('33')).toBeNull();
    expect(prefixToMask(23)).toBe('255.255.254.0');
  });

  it('catches the classic mistakes', () => {
    expect(problems('192.168.10.21', 24, '192.168.10.1')).toEqual([]);
    expect(problems('192.168.10.0', 24, '')).toEqual(['192.168.10.0 is the network address of its subnet']);
    expect(problems('192.168.10.255', 24, '')[0]).toContain('broadcast');
    expect(problems('192.168.10.21', 24, '192.168.11.1')[0]).toContain('outside 192.168.10.0/24');
    expect(problems('192.168.10.21', 24, '192.168.10.21')[0]).toContain('itself');
    expect(problems('10.0.0.300', 8, '')[0]).toContain('not an IPv4');
  });

  it('suggests a free address in a subnet', () => {
    const taken = new Set(['192.168.10.20', '192.168.10.10', '192.168.10.100']);
    expect(suggest('192.168.10.20', 24, 21, taken)).toBe('192.168.10.21');
    expect(suggest('192.168.10.20', 24, 10, taken)).toBe('192.168.10.101');
    expect(suggest('192.168.10.20', 24, null, taken)).toBe('192.168.10.101');
  });

  it('counts through a subnet and stops at its edge', () => {
    expect(nth('192.168.10.21', 3, 24)).toBe('192.168.10.24');
    expect(nth('192.168.10.253', 2, 24)).toBeNull();
    expect(inSubnet('10.20.1.5', '10.20.0.1', 23)).toBe(true);
  });
});
