mod cell;
mod context;
mod cross_ref;
mod data;
mod fmesh;
mod sdef;
mod structure;
mod surface;
pub mod types;

pub use types::{ErrorCode, Severity, ValidationCategory, ValidationError};

use crate::ast::McnpInput;
use crate::CardLineSpans;
use context::ValidationContext;

/// Validates a parsed [`McnpInput`] AST and returns all detected errors and warnings.
pub fn validate_mcnp_input(input: &McnpInput, spans: &CardLineSpans) -> Vec<ValidationError> {
    let mut ctx = ValidationContext::new(input, spans);

    structure::check_block_structure(&mut ctx);
    structure::build_lookup_maps(&mut ctx);
    surface::validate_surfaces(&mut ctx);
    cell::validate_cells(&mut ctx);
    data::validate_data_cards(&mut ctx);
    cross_ref::validate_cross_references(&mut ctx);

    ctx.diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_mcnp_input_lossy;

    #[test]
    fn test_valid_input_has_no_errors() {
        let sample = "\
Valid Input Test
1  1 -1.0  -1   imp:n=1.0
2  0        1   imp:n=0.0

1  so 10.0

mode n
sdef pos=0 0 0 erg=d1 par=n cel=1
si1  h 0.0 1.0 10.0
sp1  d 0.0 0.5 0.5
m1   1001.80c 2.0 8016.80c 1.0
mt1  lwtr.10t
f4:n 1
e4   0.1 1.0 10.0
nps  10000
";
        let res = parse_mcnp_input_lossy(sample);
        assert!(res.errors.is_empty());
        let diags = validate_mcnp_input(&res.input, &res.spans);
        assert!(diags.is_empty(), "Expected no diagnostics, got: {:?}", diags);
    }

    #[test]
    fn test_duplicate_ids_and_undefined_references() {
        let sample = "\
Error Detection Test
1  1 -1.0  -10 -99  imp:n=1.0
1  2 -2.0   10 #88  imp:n=1.0

10 so 5.0
10 so 8.0

mode n
m1 1001.80c 1.0
sdef pos=0 0 0 erg=d5
nps 1000
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::DuplicateCellId));
        assert!(codes.contains(&ErrorCode::DuplicateSurfaceId));
        assert!(codes.contains(&ErrorCode::UndefinedSurface)); // -99
        assert!(codes.contains(&ErrorCode::UndefinedCell)); // #88
        assert!(codes.contains(&ErrorCode::UndefinedMaterial)); // M2
        assert!(codes.contains(&ErrorCode::UndefinedSourceDistribution)); // D5
    }

    #[test]
    fn test_surface_and_macrobody_errors() {
        let sample = "\
Surface Error Test
1  0  -1 -2.7 -3.1  imp:n=1.0
2  0   1            imp:n=0.0

1  so -5.0
2  rpp 10.0 5.0  -1.0 1.0  -1.0 1.0
3  px 1.0 2.0

mode n
sdef pos=0 0 0
nps 100
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::InvalidSurfaceGeometry)); // SO -5.0 and RPP xmin > xmax
        assert!(codes.contains(&ErrorCode::InvalidSurfaceParamCount)); // PX with 2 params
        assert!(codes.contains(&ErrorCode::InvalidMacrobodyFacet)); // -2.7 on RPP (1..=6) and -3.1 on PX
    }

    #[test]
    fn test_circular_references_and_material_signs() {
        let sample = "\
Cycle & Material Error Test
1  1 -1.0  -1 #2  imp:n=1.0
2  1 -1.0   1 #1  imp:n=0.0

1  so 5.0

mode n
m1 1001.80c 0.5 8016.80c -0.5
sdef pos=0 0 0
kcode 1000 1.0 10 50
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::CircularCellReference));
        assert!(codes.contains(&ErrorCode::MixedMaterialFractionSigns));
        assert!(codes.contains(&ErrorCode::ConflictingSourceDefinitions));
    }

    #[test]
    fn test_tally_and_source_distribution_validation() {
        let sample = "\
Tally & Source Validation Test
1  0  -1  imp:n=1.0
2  0   1  imp:n=0.0

1  so 10.0

mode n
sdef pos=0 0 0 erg=d1
si1  h 10.0 5.0 20.0
sp1  d 0.5 0.5
f4:p 99
e4   10.0 1.0
c4   -1.5 0.0 1.0
de14 0.1 1.0
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::NonMonotonicSourceBins)); // SI1 10.0 > 5.0
        assert!(codes.contains(&ErrorCode::SourceDistributionLengthMismatch)); // SP1 len 2 vs SI1 len 3
        assert!(codes.contains(&ErrorCode::ParticleNotInMode)); // F4:p when MODE n
        assert!(codes.contains(&ErrorCode::UndefinedCell)); // F4 target 99
        assert!(codes.contains(&ErrorCode::NonMonotonicTallyBins)); // E4 10.0 > 1.0
        assert!(codes.contains(&ErrorCode::InvalidCosineBinRange)); // C4 -1.5
        assert!(codes.contains(&ErrorCode::OrphanTallyModifierCard)); // DE14 without F14
        assert!(codes.contains(&ErrorCode::IncompleteDoseCards)); // DE14 without DF14
    }

    #[test]
    fn test_universe_transform_cell_params_and_warnings() {
        let sample = "\
Universe & Cell Param Validation Test
1  1  0.0  -1  u=1 fill=2 vol=-10.0 vol=5.0 lat=3 trcl=99 imp:n=1.0
2  0        1  u=2 fill=1                                  imp:n=0.0
3  0       -2  fill=999

1  so 10.0
2  5 so 20.0
3  so 30.0

mode n
m1   1001.80c 1.0
m2   8016.80c 1.0
tr1  0 0 1 2
fmesh4:n geom=xyz imesh=10 5 iints=0
unknowncard 1 2 3
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::ZeroDensityNonVoidCell)); // Cell 1 density 0.0
        assert!(codes.contains(&ErrorCode::DuplicateCellParameter)); // duplicate VOL
        assert!(codes.contains(&ErrorCode::InvalidCellParameterValue)); // VOL=-10.0, LAT=3
        assert!(codes.contains(&ErrorCode::UndefinedTransform)); // TRCL=99 and Surface 2 TR5
        assert!(codes.contains(&ErrorCode::CircularUniverseFill)); // U=1 <-> U=2
        assert!(codes.contains(&ErrorCode::UndefinedUniverse)); // FILL=999
        assert!(codes.contains(&ErrorCode::MissingCellImportance)); // Cell 3 missing IMP:n
        assert!(codes.contains(&ErrorCode::UnusedSurface)); // Surface 3 unused
        assert!(codes.contains(&ErrorCode::UnusedMaterial)); // M2 unused
        assert!(codes.contains(&ErrorCode::InvalidTransformCardLength)); // TR1 has 4 params
        assert!(codes.contains(&ErrorCode::InvalidFmeshConfiguration)); // FMESH4 imesh 10 > 5 & iints=0
        assert!(codes.contains(&ErrorCode::UnrecognizedDataCard)); // unknowncard
        assert!(codes.contains(&ErrorCode::MissingSourceDefinition)); // no SDEF or KCODE
    }

    #[test]
    fn test_user_reported_exterior_graveyard_cell() {
        let sample = "\
Cell Card Validation - Exterior Graveyard
1   1 -1.0 -90                 imp:n=1.0 imp:p=1.0
99  0       90                 imp:n=0.0 imp:p=0.0 Exterior graveyard

90  so 100.0

mode n p
m1  1001.80c 1.0
sdef pos=0 0 0
";
        let res = parse_mcnp_input_lossy(sample);
        assert!(res.errors.is_empty(), "Parse errors: {:?}", res.errors);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let unrecognized: Vec<_> = diags
            .iter()
            .filter(|d| d.code == ErrorCode::UnrecognizedCellParameter)
            .collect();
        assert_eq!(
            unrecognized.len(),
            1,
            "Expected 1 UnrecognizedCellParameter diagnostic, got: {:?}",
            diags
        );
        assert_eq!(unrecognized[0].severity, Severity::Error);
        assert!(unrecognized[0].message.contains("exterior"));
    }

    #[test]
    fn test_enhanced_cell_card_validation() {
        let sample = "\
Enhanced Cell Card Validation Test
1  0  -10  rho=-1.0  imp:n=1.0
2  1 -2.7  -20  fill=1  imp:n=1.0
3  0  -30  lat=1  imp:n=1.0
4  0  #2   imp:n=1.0
5  0  -40  u=-5  fill=0  nonu=3  pd=1.5  pwt=-0.5  fcl=2.0  wwn=-1.0  dxc=1.5  elpt=-1.0  imp:n=1.0

10  so 10.0
20  so 20.0
30  so 30.0
40  so 40.0

mode n
m1  1001.80c 1.0
sdef pos=0 0 0
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::VoidCellWithDensity)); // Cell 1 specifies rho=-1.0 for void cell
        assert!(codes.contains(&ErrorCode::FilledCellWithMaterial)); // Cell 2 has fill=1 with material 1
        assert!(codes.contains(&ErrorCode::LatticeCellMissingFill)); // Cell 3 has lat=1 without fill
        assert!(codes.contains(&ErrorCode::InvalidCellComplement)); // Cell 4 complements filled cell #2
        assert!(codes.contains(&ErrorCode::InvalidCellParameterValue)); // Cell 5 u=-5, fill=0, nonu=3, etc.
    }

    #[test]
    fn test_vector_source_distribution_pos_axs_vec() {
        // 1. Valid 3D vector distribution without explicit `L` option (2 points = 6 entries in SI2, 2 in SP2)
        let valid_without_l = "\
Valid POS=D2 without L
1  0  -1  imp:n=1.0
2  0   1  imp:n=0.0

1  so 10.0

mode n
sdef pos=d2
si2   0.0 0.0 0.0 1.0 1.0 1.0
sp2 1 1
";
        let res = parse_mcnp_input_lossy(valid_without_l);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        assert!(
            diags.is_empty(),
            "Expected no errors for valid POS=D2 without L, got: {:?}",
            diags
        );

        // 2. Valid 3D vector distribution with explicit `L` option and second-level sampling (SI S)
        let valid_with_l_and_nested = "\
Valid POS=D2 with L and nested SI S
1  0  -1  imp:n=1.0
2  0   1  imp:n=0.0

1  so 10.0

mode n
sdef pos=d2 axs=d3
si2  l  0.0 0.0 0.0  1.0 1.0 1.0
sp2     0.5          0.5
sb2     0.5          0.5
si3  s  10 11
sp3     0.4 0.6
si10    0 0 1  0 1 0
sp10    1      1
si11 l  1 0 0  0 0 -1  0 -1 0
sp11    1      1       1
";
        let res2 = parse_mcnp_input_lossy(valid_with_l_and_nested);
        let diags2 = validate_mcnp_input(&res2.input, &res2.spans);
        assert!(
            diags2.is_empty(),
            "Expected no errors for nested vector distributions, got: {:?}",
            diags2
        );

        // 3. Invalid 3D vector distribution where SI length != 3 * SP length
        let invalid_len = "\
Invalid POS=D2 length
1  0  -1  imp:n=1.0
2  0   1  imp:n=0.0

1  so 10.0

mode n
sdef pos=d2
si2   0.0 0.0 0.0 1.0
sp2 1 1
";
        let res3 = parse_mcnp_input_lossy(invalid_len);
        let diags3 = validate_mcnp_input(&res3.input, &res3.spans);
        assert_eq!(diags3.len(), 1);
        assert_eq!(diags3[0].code, ErrorCode::SourceDistributionLengthMismatch);
    }

    #[test]
    fn test_fmesh_comprehensive_validation() {
        // 1. Valid cylindrical FMESH (MCNP Manual Listing 5.60) + energy-modified *FMESH14 with new keywords
        let valid_fmesh = "\
Valid Cylindrical and XDMF FMESH Test
1  0  -1  imp:n=1.0
2  0   1  imp:n=0.0

1  so 500.0

mode n
sdef pos=0 0 0
tr1 0 0 10
fc4 Cylindrical FMESH tally example
fmesh4:n geom=cyl origin=-100 0 0
         imesh=5 10 iints=5 2
         jmesh=100 200 jints=10 5
         kmesh=0.5 1 kints=1 2
         axs=1 0 0 vec=0 1 0
         out=xdmf
*fmesh14:n geom=xyz origin=-10 -10 -10
           imesh=10 iints=5
           jmesh=10 jints=5
           kmesh=10 kints=5
           emesh=0.1 1.0 10.0 eints=1 2 5 enorm=yes
           tmesh=0.0 100.0 tints=1 5 tnorm=no
           factor=2.0 out=none tr=1 inc=0 infinite type=source kclear=0 tally=fast_hist
";
        let res = parse_mcnp_input_lossy(valid_fmesh);
        assert!(res.errors.is_empty(), "Parse errors: {:?}", res.errors);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        assert!(
            diags.is_empty(),
            "Expected no validation errors for valid FMESH deck, got: {:?}",
            diags
        );

        // 2. Invalid FMESH configurations
        let invalid_fmesh = "\
Invalid FMESH Configurations
1  0  -1  imp:n=1.0
2  0   1  imp:n=0.0

1  so 100.0

mode n p
ssr
f4:n 1
fmesh4:n geom=xyz imesh=10 jmesh=10 kmesh=10 out=xdmf
fmesh5:n geom=xyz imesh=10 jmesh=10 kmesh=10 out=col
fmesh14:n,p geom=xyz origin=0 0 0 imesh=-5 10 iints=5 jmesh=10 kmesh=10 tr=99
fmesh24:n geom=cyl axs=1 0 0 imesh=5 jmesh=10 kmesh=0.5
fmesh34:n geom=xyz imesh=10 jmesh=10 kmesh=10 tally=batch out=none
fmesh44:n geom=xyz imes=10 jmesh=10 kmesh=10 out=none
e14 1.0 10.0
";
        let res_inv = parse_mcnp_input_lossy(invalid_fmesh);
        let diags_inv = validate_mcnp_input(&res_inv.input, &res_inv.spans);

        // Check F4 vs FMESH4 duplicate ID (E106)
        assert!(diags_inv.iter().any(|d| d.code == ErrorCode::DuplicateDataCard
            && d.message.contains("FMESH4 conflicts with tally F4")));
        // Check undefined transform TR99 on FMESH14 (E306)
        assert!(diags_inv.iter().any(|d| d.code == ErrorCode::UndefinedTransform
            && d.message.contains("FMESH14 references undefined transform TR99")));
        // Check ID % 10 != 4 on FMESH5
        assert!(diags_inv
            .iter()
            .any(|d| d.code == ErrorCode::InvalidFmeshConfiguration
                && d.message.contains("FMESH5 ID must end with digit 4")));
        // Check XDMF mixed with out=col on FMESH5
        assert!(diags_inv
            .iter()
            .any(|d| d.code == ErrorCode::InvalidFmeshConfiguration
                && d.message
                    .contains("FMESH5 output format `col` cannot be combined with XDMF")));
        // Check multiple particles on FMESH14:n,p
        assert!(diags_inv
            .iter()
            .any(|d| d.code == ErrorCode::InvalidFmeshConfiguration
                && d.message
                    .contains("FMESH14 allows only a single particle designator")));
        // Check imesh[0] <= origin.x on FMESH14
        assert!(diags_inv
            .iter()
            .any(|d| d.code == ErrorCode::InvalidFmeshConfiguration
                && d.message.contains(
                    "FMESH14 I (IMESH/IINTS) first coarse mesh point (-5) must be greater than ORIGIN X (0)"
                )));
        // Check imesh.len() != iints.len() on FMESH14 (2 vs 1)
        assert!(diags_inv
            .iter()
            .any(|d| d.code == ErrorCode::InvalidFmeshConfiguration
                && d.message.contains(
                    "FMESH14 I (IMESH/IINTS) has 2 mesh points but 1 interval counts"
                )));
        // Check parallel AXS and default VEC=(1,0,0) on FMESH24
        assert!(diags_inv
            .iter()
            .any(|d| d.code == ErrorCode::InvalidFmeshConfiguration
                && d.message
                    .contains("FMESH24 AXS and VEC vectors must not be parallel")));
        // Check cylindrical KMESH not ending with 1.0 on FMESH24
        assert!(diags_inv
            .iter()
            .any(|d| d.code == ErrorCode::InvalidFmeshConfiguration
                && d.message.contains(
                    "FMESH24 cylindrical KMESH (theta in revolutions) must end with 1.0"
                )));
        // Check SSR + tally=batch incompatibility on FMESH34
        assert!(diags_inv
            .iter()
            .any(|d| d.code == ErrorCode::InvalidFmeshConfiguration
                && d.message
                    .contains("FMESH34 TALLY=batch is incompatible with an SSR")));
        // Check typo keyword `imes=10` after `geom=xyz` on FMESH44
        assert!(diags_inv
            .iter()
            .any(|d| d.code == ErrorCode::InvalidFmeshConfiguration
                && d.message.contains("FMESH44 keyword `GEOM` has unexpected extra or unrecognized token(s): `imes 10`")));
        // Check E14 used with FMESH14
        assert!(diags_inv
            .iter()
            .any(|d| d.code == ErrorCode::InvalidFmeshConfiguration
                && d.message.contains("E14 cannot be used with mesh tally FMESH14")));
    }

    #[test]
    fn test_sdef_comprehensive_validation() {
        let valid_sdef = "\
SDEF valid test
1 1 -1.0 -1 imp:n=1
2 0 1 imp:n=0

1 so 10.0

mode n
m1 92235.80c 1.0
sdef pos=0 0 0 erg=14.1 dir=0.5 rad=2.0 wgt=1.0 eff=0.01 nrm=-1 cel=1 par=n
nps 100
";
        let res = parse_mcnp_input_lossy(valid_sdef);
        assert!(res.errors.is_empty(), "parse errors: {:?}", res.errors);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);

        let valid_special_par = "\
SDEF cosmic/background test
1 1 -1.0 -1 imp:n=1
2 0 1 imp:n=0

1 so 10.0

mode n
m1 92235.80c 1.0
sdef pos=0 0 0 rad=100.0 par=BG loc=35.0 139.0 0.1 dat=6 15 2020
nps 100
";
        let res_sp = parse_mcnp_input_lossy(valid_special_par);
        assert!(res_sp.errors.is_empty(), "parse errors: {:?}", res_sp.errors);
        let diags_sp = validate_mcnp_input(&res_sp.input, &res_sp.spans);
        assert!(
            diags_sp.is_empty(),
            "expected no diagnostics for special PAR=BG, got: {:?}",
            diags_sp
        );

        let invalid_sdef = "\
SDEF invalid test
1 1 -1.0 -1 imp:n=1
2 0 1 imp:n=0

1 so 10.0

mode n
m1 92235.80c 1.0
sdef erg=-1.0 erg=2.0 dir=1.5 rad=-0.5 wgt=d1 eff=1.2 nrm=2 ccc=999 cel=fpos d2 erg=frad d3 rad=fcel d4 tr=d5
si1 1.0 2.0
sp1 0.5 0.5
si2 1
sp2 1.0
si3 1.0 2.0
sp3 0.5 0.5
si4 1.0 2.0
sp4 0.5 0.5
si5 h 1 2
sp5 0.0 1.0
nps 100
";
        let res_inv = parse_mcnp_input_lossy(invalid_sdef);
        let diags_inv = validate_mcnp_input(&res_inv.input, &res_inv.spans);
        let has_code_msg = |code: ErrorCode, substr: &str| {
            diags_inv
                .iter()
                .any(|d| d.code == code && d.message.contains(substr))
        };

        assert!(has_code_msg(ErrorCode::InvalidSdefParameter, "Duplicate SDEF keyword `ERG`"));
        assert!(has_code_msg(ErrorCode::InvalidSdefParameter, "SDEF ERG=-1 cannot be negative"));
        assert!(has_code_msg(ErrorCode::InvalidSdefParameter, "SDEF DIR=1.5 must be in range [-1, 1]"));
        assert!(has_code_msg(ErrorCode::InvalidSdefParameter, "SDEF RAD=-0.5 cannot be negative"));
        assert!(has_code_msg(ErrorCode::InvalidSdefParameter, "SDEF WGT must be an explicit scalar value"));
        assert!(has_code_msg(ErrorCode::InvalidSdefParameter, "SDEF EFF=1.2 must be in range (0, 1]"));
        assert!(has_code_msg(ErrorCode::InvalidSdefParameter, "SDEF NRM=2 must be +1 or -1"));
        assert!(has_code_msg(ErrorCode::UndefinedCell, "SDEF CCC=999 references undefined cell 999"));
        assert!(has_code_msg(ErrorCode::InvalidSdefDependency, "cannot depend on POS (FPOS)"));
        assert!(has_code_msg(ErrorCode::InvalidSdefDependency, "only one level of dependence is allowed"));
        assert!(has_code_msg(ErrorCode::InconsistentSdefOptions, "requires SI5 to use option L"));

        // Duplicate Dn usage and PAR=BG without LOC
        let invalid_sdef2 = "\
SDEF invalid test 2
1 1 -1.0 -1 imp:n=1
2 0 1 imp:n=0

1 so 10.0

mode n
m1 92235.80c 1.0
sdef erg=d1 rad=d1 par=BG
si1 1.0 2.0
sp1 0.5 0.5
nps 100
";
        let res_inv2 = parse_mcnp_input_lossy(invalid_sdef2);
        let diags_inv2 = validate_mcnp_input(&res_inv2.input, &res_inv2.spans);
        assert!(diags_inv2.iter().any(|d| {
            d.code == ErrorCode::InvalidSdefDependency
                && d.message.contains("Distribution D1 is used by multiple SDEF variables")
        }));
        assert!(diags_inv2.iter().any(|d| {
            d.code == ErrorCode::InconsistentSdefOptions
                && d.message.contains("SDEF PAR=BG/BN/BP requires the LOC keyword")
        }));
    }
}
