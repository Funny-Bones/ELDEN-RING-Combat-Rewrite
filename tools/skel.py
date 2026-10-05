"""Forward kinematics helpers shared by the animation baker."""


def qmul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )


def qrot(q, v):
    x, y, z, w = q
    vx, vy, vz = v
    tx = 2 * (y * vz - z * vy)
    ty = 2 * (z * vx - x * vz)
    tz = 2 * (x * vy - y * vx)
    return (vx + w * tx + (y * tz - z * ty), vy + w * ty + (z * tx - x * tz), vz + w * tz + (x * ty - y * tx))


def fk(parents, local):
    """Model-space (position, rotation) for every bone. Parents precede children."""
    out = [None] * len(local)
    for i, (p, q) in enumerate(local):
        if parents[i] < 0:
            out[i] = (p, q)
        else:
            pp, pq = out[parents[i]]
            rp = qrot(pq, p)
            out[i] = ((pp[0] + rp[0], pp[1] + rp[1], pp[2] + rp[2]), qmul(pq, q))
    return out
