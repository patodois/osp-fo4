# Rotation convention and arithmetic

The row convention Rz*Ry*Rx and transposed application are supported by fourteen
measured roots, uniquely among 24 candidates below 1e-5 (maximum 2.1e-7; runner-up
1.43). This does not establish binary32 evaluation order or trigonometric math.
Only identity currently matches all 13 transform words.

The exact arithmetic investigation is limited to two hours and the supplied
rotation measurement document, without reading a game executable. No numerical
candidate changes the renderer unless every matrix word is reproduced and the
new rule is explicitly reviewed. Results will be recorded here.

The bounded investigation did not close the arithmetic. The supplied matrix
report does not contain the original Euler inputs. Conditional inverse inputs
were recovered from its six frozen order predictions, with periodic branches
and nearby binary32 representatives checked against all 54 predicted words.
46 original experiments compared binary32/binary64 sin/cos, Hamilton products,
three quaternion-to-matrix diagonals with/without normalization, and Taylor
series (3 through 10 terms, with/without quadrant reflection). The best new
conditional experiment matched 69/126 words (3/14 matrices), maximum absolute
error 2.3841858e-7. Inverse inputs are not asserted to be the authored inputs;
this result cannot establish a runtime arithmetic law. No implementation was
changed. The frozen direct prediction remains separately recorded. All numeric
input came from the rotation report; no executable or other measurement input
was read by the search. The search stopped well within the two-hour ceiling.
