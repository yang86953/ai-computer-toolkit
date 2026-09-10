#pragma once

namespace act::components {

enum class StaticMetadataAccess {
    available,
    permission_blocked,
    unavailable,
};

enum class StaticIntegrityRelation {
    lower,
    same,
    higher,
    unknown,
};

struct StaticPermissionAssessment {
    const char* decision;
    const char* permission_relation;
    bool safe_to_execute_now;
    bool requires_confirmation;
    bool foreground_required;
    bool active_write_probe_performed;
};

[[nodiscard]] StaticPermissionAssessment
assess_static_background_mutation(
    StaticMetadataAccess metadata_access,
    StaticIntegrityRelation integrity_relation);

}  // namespace act::components
