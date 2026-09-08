// A minimal cmi5 package, built in-process so the cmi5 stage needs no committed
// binary fixture and no `zip` tool on the guest.
//
// The archive is written with the STORE method (no compression), which the
// server's `zip` crate reads directly. That keeps the writer here small and
// deterministic: a local file header + data per entry, then a central directory,
// then the end-of-central-directory record.

import { Buffer } from 'node:buffer'

/// Standard CRC-32 (IEEE), bit-by-bit. Small and dependency-free; the archive is
/// tiny so speed does not matter.
function crc32(bytes) {
  let crc = 0xffffffff
  for (let i = 0; i < bytes.length; i++) {
    crc ^= bytes[i]
    for (let j = 0; j < 8; j++) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1))
    }
  }
  return (crc ^ 0xffffffff) >>> 0
}

/// Build a STORE-method zip from `[{ name, data }]`, where `data` is a string or
/// Buffer. Returns a Buffer.
export function buildPackageZip(entries) {
  const files = entries.map((e) => ({
    name: Buffer.from(e.name, 'utf8'),
    data: Buffer.isBuffer(e.data) ? e.data : Buffer.from(e.data, 'utf8'),
  }))

  const parts = []
  const central = []
  let offset = 0

  for (const f of files) {
    const crc = crc32(f.data)
    const local = Buffer.alloc(30)
    local.writeUInt32LE(0x04034b50, 0) // local file header signature
    local.writeUInt16LE(20, 4) // version needed
    local.writeUInt16LE(0, 6) // flags
    local.writeUInt16LE(0, 8) // method: store
    local.writeUInt16LE(0, 10) // mod time
    local.writeUInt16LE(0, 12) // mod date
    local.writeUInt32LE(crc, 14)
    local.writeUInt32LE(f.data.length, 18) // compressed size
    local.writeUInt32LE(f.data.length, 22) // uncompressed size
    local.writeUInt16LE(f.name.length, 26)
    local.writeUInt16LE(0, 28) // extra length
    parts.push(local, f.name, f.data)

    const cd = Buffer.alloc(46)
    cd.writeUInt32LE(0x02014b50, 0) // central directory header signature
    cd.writeUInt16LE(20, 4) // version made by
    cd.writeUInt16LE(20, 6) // version needed
    cd.writeUInt16LE(0, 8) // flags
    cd.writeUInt16LE(0, 10) // method: store
    cd.writeUInt16LE(0, 12) // mod time
    cd.writeUInt16LE(0, 14) // mod date
    cd.writeUInt32LE(crc, 16)
    cd.writeUInt32LE(f.data.length, 20)
    cd.writeUInt32LE(f.data.length, 24)
    cd.writeUInt16LE(f.name.length, 28)
    cd.writeUInt16LE(0, 30) // extra length
    cd.writeUInt16LE(0, 32) // comment length
    cd.writeUInt16LE(0, 34) // disk number start
    cd.writeUInt16LE(0, 36) // internal attrs
    cd.writeUInt32LE(0, 38) // external attrs
    cd.writeUInt32LE(offset, 42) // local header offset
    central.push(cd, f.name)

    offset += local.length + f.name.length + f.data.length
  }

  const cdStart = offset
  const cdSize = central.reduce((n, b) => n + b.length, 0)

  const eocd = Buffer.alloc(22)
  eocd.writeUInt32LE(0x06054b50, 0) // end of central directory signature
  eocd.writeUInt16LE(0, 4) // disk number
  eocd.writeUInt16LE(0, 6) // cd start disk
  eocd.writeUInt16LE(files.length, 8)
  eocd.writeUInt16LE(files.length, 10)
  eocd.writeUInt32LE(cdSize, 12)
  eocd.writeUInt32LE(cdStart, 16)
  eocd.writeUInt16LE(0, 20) // comment length

  return Buffer.concat([...parts, ...central, eocd])
}

/// A single-AU cmi5 package. moveOn=Passed with masteryScore 0.8, so one passing
/// statement at or above 0.8 satisfies it. IRIs are unique per run so repeated
/// battery runs (which do not reset between imports within a run) do not collide.
export function minimalPackage(tag) {
  const courseIri = `https://e2e.invalid/cmi5/course/${tag}`
  const auIri = `https://e2e.invalid/cmi5/au/${tag}`
  const manifest = `<?xml version="1.0" encoding="UTF-8"?>
<courseStructure xmlns="https://w3id.org/xapi/profiles/cmi5/v1/CourseStructure.xsd">
  <course id="${courseIri}">
    <title><langstring lang="en-US">E2E Safety Course</langstring></title>
    <description><langstring lang="en-US">A minimal cmi5 course for the e2e battery.</langstring></description>
  </course>
  <au id="${auIri}" moveOn="Passed" masteryScore="0.8">
    <title><langstring lang="en-US">Safety Basics</langstring></title>
    <url>index.html</url>
  </au>
</courseStructure>`
  const index = '<!doctype html><title>E2E cmi5 AU</title><p>hello</p>'
  const zip = buildPackageZip([
    { name: 'cmi5.xml', data: manifest },
    { name: 'index.html', data: index },
  ])
  return { zip, courseIri, auIri }
}
