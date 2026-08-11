/*
 * Embench 1.0's cubic benchmark uses 128-bit long double. The repository's
 * prebuilt libgcc is medlow and cannot be linked into an M-mode image at
 * 0x80000000, so this port uses the same public-domain solver with double.
 */

#include <math.h>

#define PI 3.14159265358979323846264338327950288

void
SolveCubic(double a, double b, double c, double d, int *solutions, double *x)
{
    double a1 = b / a;
    double a2 = c / a;
    double a3 = d / a;
    double q = (a1 * a1 - 3.0 * a2) / 9.0;
    double r = (2.0 * a1 * a1 * a1 - 9.0 * a1 * a2 + 27.0 * a3) / 54.0;
    double discriminant = r * r - q * q * q;

    if (discriminant <= 0.0) {
        double theta = acos(r / sqrt(q * q * q));
        *solutions = 3;
        x[0] = -2.0 * sqrt(q) * cos(theta / 3.0) - a1 / 3.0;
        x[1] = -2.0 * sqrt(q) * cos((theta + 2.0 * PI) / 3.0) - a1 / 3.0;
        x[2] = -2.0 * sqrt(q) * cos((theta + 4.0 * PI) / 3.0) - a1 / 3.0;
    } else {
        x[0] = pow(sqrt(discriminant) + __builtin_fabs(r), 1.0 / 3.0);
        x[0] += q / x[0];
        x[0] *= r < 0.0 ? 1.0 : -1.0;
        x[0] -= a1 / 3.0;
        *solutions = 1;
    }
}
