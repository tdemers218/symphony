# Rigs and deformers

A rig lets you pose a character instead of redrawing it. In Symphony a rig
is a **Skeleton** with **Bones**; drawings connected to a bone follow it,
and vector artwork bends at the joints. A **Mesh Warp** bends artwork with
points instead.

## Build a skeleton

1. Search **Add Skeleton** or **Add Bone** (`Tab`). You get a Skeleton
   with one Bone connected. The Skeleton moves the whole rig.
2. Select a Bone and choose **Add Bone** again to extend the chain.
3. Double-click the Skeleton in the Node view to open its setup view: the
   bones in their rest pose, each with a thumbnail of its drawing. Drag a
   base or a tip to reshape the rig; a joint is one point, so dragging it
   moves both bones that meet there. Press and hold an endpoint, then
   drag, to grow a new bone from it. Shift-click adds bones.

## Set the rest pose

In **Bone properties → Rest pose and deformation**:

- **Length** and **Rest angle** place the bone over the artwork.
- **Root origin** (first bone only) sets where the chain starts.
- **Bend radius**: how softly the artwork bends at this joint.
- **Stretch**, with a **Limit** of **Shortest ×** and **Longest ×**.
  Resistance grows as the bone nears either limit.

## Attach artwork

Connect a Bone's output to one or more Drawing inputs. To keep a drawing's
existing peg, connect the Bone to that peg instead. Vector artwork bends
at the joint; imported pictures follow their bone rigidly.

## Pose the rig

Select a Bone and use the **Transform** tool in Camera.

- Drag a bone's tip to pose it. `Shift` snaps to 22.5° steps. Arrow keys
  nudge.
- Right-click the joint where two bones meet and choose **Make smart
  joint**. Drag the square target at the end of the second bone to pose
  both bones together.
- **Flip bend** (`F`, or in Bone properties) chooses which way an elbow
  or knee bends.
- **Anchors** keep a hand or foot in place while the rest of the body
  moves. `A` cycles a joint's anchor mode: **Locked** keeps the joint on
  its target; **Aim** lets the target guide the pose without pinning it.
- Animate with **Auto-key** or `K` over the Timeline. Bones can be
  animated without artwork, and never appear in exports.

## Mesh Warp

A Mesh Warp bends the artwork connected under it. Add it from the Node
view or search, select it, and use the **Transform** tool. Choose its
**Kind** in Node Properties:

- **Grid**: a grid over the artwork. Drag a grid point to bend everything
  under it.
- **Points**: click the artwork to place points (`Alt`-click removes
  one), then click **Build mesh**. A mesh is made over the artwork's
  shape, and dragging a point bends the art as rigidly as it can, moving
  only what's connected to it. **Edit points** goes back to placing them.

With Auto-key on, a move keys the point on the current frame; otherwise it
changes every frame. One drag is one undo step.

## Still to come

Multi-bone weights, rig templates, retargeting animation between
characters, and Curve and Envelope deformers are **Planned**. See
[Planned features](planned.md).
