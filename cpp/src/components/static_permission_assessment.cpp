#include "components/static_permission_assessment.hpp"

namespace act::components {

StaticPermissionAssessment
assess_static_background_mutation(
    const StaticMetadataAccess metadata_access,
    const StaticIntegrityRelation integrity_relation) {
    const auto result = [](const char* decision,
                           const char* permission_relation) {
        return StaticPermissionAssessment{
            decision,
            permission_relation,
            false,
            true,
            false,
            false,
        };
    };
    if (metadata_access ==
        StaticMetadataAccess::permission_blocked) {
        return result(
            "permission-blocked",
            "target-metadata-permission-blocked");
    }
    if (metadata_access ==
        StaticMetadataAccess::unavailable) {
        return result("indeterminate", "unknown");
    }
    if (integrity_relation ==
        StaticIntegrityRelation::higher) {
        return result(
            "permission-blocked",
            "target-higher-integrity");
    }
    if (integrity_relation ==
        StaticIntegrityRelation::unknown) {
        return result("indeterminate", "unknown");
    }
    return result(
        "requires-confirmation",
        "no-static-integrity-block-observed");
}

}  // namespace act::components
