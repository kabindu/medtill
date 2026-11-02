//! What MedTill knows about medicines before anyone teaches it: generic
//! names and brands sold in Kenyan pharmacies, grouped by category, plus the
//! words found on every box that are *not* the medicine (makers, dosage
//! forms, instructions). The box reader (`scanner`) uses this to tell the
//! name apart from the rest of the print and to suggest a category.
//!
//! Everything is upper case. A name with a space ("MARA MOJA") is matched as
//! a phrase; any other name word by word, allowing one misread letter.

/// One category: what it is called, other words a shop might use for it
/// ("Pain relief", "Analgesics"), the medicines in it, and words that point
/// to it without being a medicine's name ("EYE DROPS").
pub struct Group {
    pub category: &'static str,
    pub also: &'static [&'static str],
    pub names: &'static [&'static str],
    pub hints: &'static [&'static str],
}

#[rustfmt::skip]
pub const GROUPS: &[Group] = &[
    // Checked first: "Chloramphenicol eye drops" belongs here, not under antibiotics.
    Group { category: "Eye & ear", also: &["EYE", "EAR", "OPHTHALMIC", "OPTHALMIC", "OTIC"],
        names: &["TOBRADEX", "MAXITROL"],
        hints: &["EYE DROPS", "EAR DROPS", "EYE OINTMENT", "EYE/EAR", "OPHTHALMIC", "OTIC"] },
    Group { category: "Painkiller", also: &["PAIN", "ANALGESIC", "ANALGESICS", "PAINKILLER", "PAINKILLERS", "NSAID", "FEVER"],
        names: &["PARACETAMOL", "ACETAMINOPHEN", "IBUPROFEN", "DICLOFENAC", "ASPIRIN", "MEFENAMIC", "NAPROXEN",
            "TRAMADOL", "MELOXICAM", "PIROXICAM", "KETOPROFEN", "ACECLOFENAC", "ETORICOXIB", "CELECOXIB",
            "INDOMETHACIN", "CODEINE", "PANADOL", "CALPOL", "HEDEX", "MARA MOJA", "BRUFEN", "NUROFEN", "CATAFLAM",
            "VOLTAREN", "DISPRIN", "PONSTAN", "DEEP HEAT", "ROBB"],
        hints: &[] },
    Group { category: "Antibiotic", also: &["ANTIBIOTIC", "ANTIBIOTICS", "ANTIBACTERIAL", "INFECTION"],
        names: &["AMOXICILLIN", "AMOXYCILLIN", "CLAVULANATE", "CLAVULANIC", "AMPICILLIN", "CLOXACILLIN",
            "FLUCLOXACILLIN", "AMPICLOX", "CIPROFLOXACIN", "LEVOFLOXACIN", "OFLOXACIN", "NORFLOXACIN", "AZITHROMYCIN",
            "ERYTHROMYCIN", "CLARITHROMYCIN", "DOXYCYCLINE", "TETRACYCLINE", "METRONIDAZOLE", "TINIDAZOLE",
            "CEFUROXIME", "CEFTRIAXONE", "CEFALEXIN", "CEPHALEXIN", "CEFIXIME", "CEFPODOXIME", "CEFADROXIL",
            "COTRIMOXAZOLE", "SULFAMETHOXAZOLE", "TRIMETHOPRIM", "NITROFURANTOIN", "GENTAMICIN", "CHLORAMPHENICOL",
            "CLINDAMYCIN", "LINEZOLID", "PHENOXYMETHYLPENICILLIN", "AMOXIL", "AUGMENTIN", "FLAGYL", "SEPTRIN",
            "ZITHROMAX", "ZINNAT", "ROCEPHIN", "KEFLEX"],
        hints: &[] },
    Group { category: "Antimalarial", also: &["MALARIA", "ANTIMALARIAL", "ANTIMALARIALS"],
        names: &["ARTEMETHER", "LUMEFANTRINE", "ARTESUNATE", "DIHYDROARTEMISININ", "PIPERAQUINE", "AMODIAQUINE",
            "QUININE", "SULFADOXINE", "PYRIMETHAMINE", "PRIMAQUINE", "ATOVAQUONE", "PROGUANIL", "MEFLOQUINE",
            "COARTEM", "LONART", "ARTEFAN", "DUO-COTECXIN", "DUOCOTECXIN", "P-ALAXIN", "PALAXIN", "FANSIDAR",
            "MALARONE"],
        hints: &[] },
    Group { category: "Antiretroviral (ARV)", also: &["ARV", "ARVS", "HIV", "ANTIRETROVIRAL", "ANTIRETROVIRALS"],
        names: &["TENOFOVIR", "LAMIVUDINE", "EFAVIRENZ", "DOLUTEGRAVIR", "ZIDOVUDINE", "NEVIRAPINE", "LOPINAVIR",
            "RITONAVIR", "ATAZANAVIR", "ABACAVIR", "EMTRICITABINE", "DARUNAVIR", "RALTEGRAVIR", "ALUVIA", "TRUVADA"],
        hints: &[] },
    Group { category: "Antiviral", also: &["ANTIVIRAL", "ANTIVIRALS", "VIRAL"],
        names: &["ACICLOVIR", "ACYCLOVIR", "VALACICLOVIR", "OSELTAMIVIR", "ZOVIRAX", "TAMIFLU"],
        hints: &[] },
    Group { category: "Antifungal", also: &["FUNGAL", "ANTIFUNGAL", "ANTIFUNGALS"],
        names: &["FLUCONAZOLE", "CLOTRIMAZOLE", "KETOCONAZOLE", "MICONAZOLE", "ITRACONAZOLE", "NYSTATIN",
            "GRISEOFULVIN", "TERBINAFINE", "DIFLUCAN", "CANESTEN", "NIZORAL", "DAKTARIN", "LAMISIL"],
        hints: &[] },
    Group { category: "Allergy", also: &["ALLERGY", "ALLERGIES", "ANTIHISTAMINE", "ANTIHISTAMINES", "ANTIALLERGIC"],
        names: &["CETIRIZINE", "LEVOCETIRIZINE", "LORATADINE", "DESLORATADINE", "CHLORPHENIRAMINE", "CHLORPHENAMINE",
            "PROMETHAZINE", "FEXOFENADINE", "CYPROHEPTADINE", "PIRITON", "ZYRTEC", "CLARITYNE", "CLARITIN",
            "PHENERGAN", "TELFAST", "PERIACTIN"],
        hints: &[] },
    Group { category: "Cough & cold", also: &["COUGH", "COLD", "FLU", "RESPIRATORY"],
        names: &["DEXTROMETHORPHAN", "GUAIFENESIN", "GUAIPHENESIN", "BROMHEXINE", "AMBROXOL", "CARBOCISTEINE",
            "PSEUDOEPHEDRINE", "PHENYLEPHRINE", "DIPHENHYDRAMINE", "BENYLIN", "MUCOSOLVAN", "ACTIFED", "TRIAMINIC",
            "LEMSIP"],
        hints: &["COUGH", "EXPECTORANT", "DECONGESTANT"] },
    Group { category: "Stomach & digestion", also: &["STOMACH", "GASTRO", "GASTROINTESTINAL", "DIGESTIVE", "DIGESTION", "ANTACID", "ANTACIDS", "ULCER", "GIT"],
        names: &["OMEPRAZOLE", "ESOMEPRAZOLE", "PANTOPRAZOLE", "LANSOPRAZOLE", "RABEPRAZOLE", "RANITIDINE",
            "FAMOTIDINE", "CIMETIDINE", "MAGNESIUM TRISILICATE", "ALUMINIUM HYDROXIDE", "SIMETHICONE", "LOPERAMIDE",
            "HYOSCINE", "METOCLOPRAMIDE", "DOMPERIDONE", "ONDANSETRON", "BISACODYL", "LACTULOSE", "LOSEC", "NEXIUM",
            "GAVISCON", "ENO", "IMODIUM", "BUSCOPAN", "MOTILIUM", "DULCOLAX", "RELCER", "MUCAINE"],
        hints: &["ANTACID", "ORAL REHYDRATION", "ORS"] },
    Group { category: "Diabetes", also: &["DIABETES", "DIABETIC", "ANTIDIABETIC", "ANTIDIABETICS", "SUGAR"],
        names: &["METFORMIN", "GLIBENCLAMIDE", "GLICLAZIDE", "GLIMEPIRIDE", "SITAGLIPTIN", "VILDAGLIPTIN",
            "PIOGLITAZONE", "INSULIN", "GLUCOPHAGE", "DAONIL", "DIAMICRON", "AMARYL", "MIXTARD", "ACTRAPID", "GALVUS",
            "JANUVIA"],
        hints: &[] },
    Group { category: "Blood pressure & heart", also: &["PRESSURE", "HYPERTENSION", "ANTIHYPERTENSIVE", "ANTIHYPERTENSIVES", "CARDIAC", "HEART", "CARDIOVASCULAR", "CHOLESTEROL", "BP"],
        names: &["AMLODIPINE", "NIFEDIPINE", "LOSARTAN", "VALSARTAN", "TELMISARTAN", "IRBESARTAN", "CANDESARTAN",
            "ENALAPRIL", "LISINOPRIL", "CAPTOPRIL", "RAMIPRIL", "PERINDOPRIL", "ATENOLOL", "BISOPROLOL", "METOPROLOL",
            "PROPRANOLOL", "CARVEDILOL", "HYDROCHLOROTHIAZIDE", "FUROSEMIDE", "SPIRONOLACTONE", "INDAPAMIDE",
            "METHYLDOPA", "HYDRALAZINE", "ATORVASTATIN", "SIMVASTATIN", "ROSUVASTATIN", "CLOPIDOGREL", "WARFARIN",
            "DIGOXIN", "NORVASC", "COZAAR", "LASIX", "ALDOMET", "LIPITOR", "CONCOR"],
        hints: &[] },
    Group { category: "Asthma", also: &["ASTHMA", "BRONCHODILATOR", "INHALER", "INHALERS"],
        names: &["SALBUTAMOL", "ALBUTEROL", "BECLOMETASONE", "BECLOMETHASONE", "BUDESONIDE", "FLUTICASONE",
            "SALMETEROL", "FORMOTEROL", "MONTELUKAST", "AMINOPHYLLINE", "THEOPHYLLINE", "IPRATROPIUM", "VENTOLIN",
            "SERETIDE", "SYMBICORT", "SINGULAIR", "BECOTIDE"],
        hints: &["INHALER"] },
    Group { category: "Dewormer", also: &["DEWORMER", "DEWORMERS", "DEWORMING", "WORMS", "ANTHELMINTIC", "ANTHELMINTICS"],
        names: &["ALBENDAZOLE", "MEBENDAZOLE", "PRAZIQUANTEL", "IVERMECTIN", "LEVAMISOLE", "PYRANTEL",
            "NICLOSAMIDE", "ZENTEL", "VERMOX", "KETRAX"],
        hints: &[] },
    Group { category: "Steroid", also: &["STEROID", "STEROIDS", "CORTICOSTEROID", "CORTICOSTEROIDS"],
        names: &["PREDNISOLONE", "PREDNISONE", "DEXAMETHASONE", "HYDROCORTISONE", "BETAMETHASONE",
            "METHYLPREDNISOLONE", "TRIAMCINOLONE", "CLOBETASOL", "BETNOVATE", "DERMOVATE"],
        hints: &[] },
    Group { category: "Family planning", also: &["FAMILY", "PLANNING", "CONTRACEPTIVE", "CONTRACEPTIVES", "CONTRACEPTION", "REPRODUCTIVE"],
        names: &["LEVONORGESTREL", "ETHINYLESTRADIOL", "ETHINYL ESTRADIOL", "DESOGESTREL", "NORETHISTERONE",
            "MEDROXYPROGESTERONE", "POSTINOR", "MICROGYNON", "DEPO-PROVERA", "FEMIPLAN", "DUREX"],
        hints: &["CONDOM", "CONDOMS", "CONTRACEPTIVE"] },
    Group { category: "Vitamins & supplements", also: &["VITAMIN", "VITAMINS", "SUPPLEMENT", "SUPPLEMENTS", "NUTRITION", "NUTRITIONAL", "MINERALS"],
        names: &["MULTIVITAMIN", "MULTIVITAMINS", "FOLIC", "FERROUS", "ASCORBIC", "CALCIUM", "ZINC", "COD LIVER",
            "OMEGA", "B-COMPLEX", "CYANOCOBALAMIN", "PYRIDOXINE", "THIAMINE", "RANFERON", "SUPRADYN", "CENTRUM",
            "SEVEN SEAS", "PREGNACARE", "FEROGLOBIN", "WELLWOMAN", "WELLMAN"],
        hints: &["VITAMIN", "SUPPLEMENT"] },
    Group { category: "Skin care", also: &["SKIN", "DERMATOLOGICAL", "DERMATOLOGY", "TOPICAL"],
        names: &["CALAMINE", "SILVER SULFADIAZINE", "BENZOYL", "SALICYLIC", "FUSIDIC", "MUPIROCIN", "PERMETHRIN",
            "BENZYL BENZOATE", "FUCIDIN", "BACTROBAN", "WHITFIELD"],
        hints: &["AQUEOUS CREAM", "EMULSIFYING"] },
    Group { category: "Mental health & sleep", also: &["MENTAL", "PSYCHIATRIC", "ANTIDEPRESSANT", "ANTIDEPRESSANTS", "SEDATIVE", "SEDATIVES", "SLEEP", "NERVES"],
        names: &["AMITRIPTYLINE", "IMIPRAMINE", "FLUOXETINE", "SERTRALINE", "ESCITALOPRAM", "PAROXETINE", "DIAZEPAM",
            "LORAZEPAM", "ALPRAZOLAM", "HALOPERIDOL", "CHLORPROMAZINE", "OLANZAPINE", "RISPERIDONE", "QUETIAPINE",
            "VALIUM", "PROZAC"],
        hints: &[] },
    Group { category: "Epilepsy", also: &["EPILEPSY", "ANTIEPILEPTIC", "ANTIEPILEPTICS", "ANTICONVULSANT", "ANTICONVULSANTS", "SEIZURE", "SEIZURES"],
        names: &["CARBAMAZEPINE", "PHENYTOIN", "VALPROATE", "VALPROIC", "PHENOBARBITONE", "PHENOBARBITAL",
            "LAMOTRIGINE", "LEVETIRACETAM", "TEGRETOL", "EPILIM"],
        hints: &[] },
    Group { category: "TB treatment", also: &["TB", "TUBERCULOSIS"],
        names: &["RIFAMPICIN", "ISONIAZID", "PYRAZINAMIDE", "ETHAMBUTOL", "RHZE"],
        hints: &[] },
    Group { category: "First aid & antiseptics", also: &["ANTISEPTIC", "ANTISEPTICS", "FIRST", "AID", "DRESSING", "DRESSINGS", "WOUND"],
        names: &["POVIDONE", "CHLORHEXIDINE", "HYDROGEN PEROXIDE", "SURGICAL SPIRIT", "METHYLATED SPIRIT",
            "GENTIAN VIOLET", "DETTOL", "SAVLON", "BETADINE"],
        hints: &["ANTISEPTIC", "BANDAGE", "GAUZE", "COTTON WOOL", "PLASTER"] },
];

/// Words on boxes that name the maker, not the medicine.
#[rustfmt::skip]
pub const MAKERS: &[&str] = &[
    "LTD", "LIMITED", "PHARMA", "PHARMACEUTICAL", "PHARMACEUTICALS", "LABORATORIES", "LABORATORY", "LABS",
    "INDUSTRIES", "HEALTHCARE", "HEALTH", "PLC", "INC", "GMBH", "CORPORATION", "CORP", "COMPANY", "MANUFACTURED",
    "MARKETED", "DISTRIBUTED", "IMPORTED", "KENYA", "INDIA", "CHINA", "NAIROBI", "MUMBAI", "GSK", "GLAXOSMITHKLINE",
    "HALEON", "PFIZER", "NOVARTIS", "SANOFI", "BAYER", "ABBOTT", "ASTRAZENECA", "RECKITT", "CIPLA", "COSMOS",
    "DAWA", "REGAL", "ELYS", "SHELYS", "RANBAXY", "AJANTA", "MACLEODS", "HETERO", "MYLAN", "VIATRIS", "ZYDUS",
    "LUPIN", "STRIDES", "GLENMARK", "SANDOZ", "TEVA", "REDDY", "REDDYS", "SUN", "AUROBINDO", "BIODEAL",
];

/// Words that are instructions, dates or batch details, never the name.
#[rustfmt::skip]
pub const INSTRUCTIONS: &[&str] = &[
    "EXP", "EXPIRY", "EXPIRES", "MFG", "MFD", "MANUFACTURING", "BATCH", "LOT", "STORE", "STORAGE", "KEEP", "DOSAGE",
    "DOSE", "DIRECTIONS", "EACH", "CONTAINS", "COMPOSITION", "REG", "PRICE", "WARNING", "CAUTION", "READ",
    "LEAFLET", "SHAKE", "PROTECT", "LIGHT", "BELOW", "ABOVE", "TEMPERATURE", "CHILDREN", "REACH", "ADULTS", "DAILY",
    "TIMES", "SWALLOW", "PRESCRIPTION", "POM", "BEFORE", "DATE", "LICENSED", "PPB", "USE", "ONLY", "SEE", "INSERT",
];

/// Advertising on the front of boxes, never the name. (Words that can be
/// part of a name, such as EXTRA, PLUS, FORTE or ADVANCE, are not here.)
#[rustfmt::skip]
pub const SLOGANS: &[&str] = &[
    "FAST", "RELIEF", "STRONG", "FORMULA", "NEW", "IMPROVED", "TRUSTED", "QUALITY", "GENUINE", "PROVEN",
    "CLINICALLY", "EFFECTIVE", "POWERFUL", "GENTLE", "LASTING", "RECOMMENDED", "DOCTOR", "DOCTORS", "BRAND",
    "NUMBER", "SINCE", "FAMILY", "PACK", "VALUE", "OFFER", "FREE", "SAVE", "BEST",
];

/// Dosage forms as printed, and how a name shows them.
#[rustfmt::skip]
pub const FORMS: &[(&str, &str)] = &[
    ("TABLETS", "Tablets"), ("TABLET", "Tablets"), ("TABS", "Tablets"), ("CAPLETS", "Caplets"),
    ("CAPSULES", "Capsules"), ("CAPSULE", "Capsules"), ("CAPS", "Capsules"), ("SYRUP", "Syrup"),
    ("SUSPENSION", "Suspension"), ("CREAM", "Cream"), ("OINTMENT", "Ointment"), ("GEL", "Gel"),
    ("INJECTION", "Injection"), ("DROPS", "Drops"), ("INHALER", "Inhaler"), ("LOTION", "Lotion"),
    ("SPRAY", "Spray"), ("SACHETS", "Sachets"), ("PESSARIES", "Pessaries"), ("SUPPOSITORIES", "Suppositories"),
    ("EFFERVESCENT", "Effervescent"), ("LOZENGES", "Lozenges"),
];

/// The group a box points to, from its texts (upper case) in order of
/// trust: the name first, then the rest of the box. Eye and ear products
/// are checked first, anywhere on the box (an antibiotic eye drop is
/// shelved with eye drops); then a medicine's name beats a hint word
/// ("Amoxil ... if cough persists" is an antibiotic, not a cough medicine).
pub fn group_of(texts: &[&str]) -> Option<&'static Group> {
    let has = |list: &[&str]| texts.iter().any(|t| list.iter().any(|n| contains_phrase(t, n)));
    let first = |pick: fn(&Group) -> &[&str]| {
        texts.iter().find_map(|t| GROUPS.iter().find(|g| pick(g).iter().any(|n| contains_phrase(t, n))))
    };
    GROUPS[..1]
        .iter()
        .find(|g| has(g.hints) || has(g.names))
        .or_else(|| first(|g| g.names))
        .or_else(|| first(|g| g.hints))
}

/// The group that lists `name`.
pub fn group_named(name: &str) -> Option<&'static Group> {
    GROUPS.iter().find(|g| g.names.contains(&name))
}

/// True if `text` holds `phrase` as whole words ("ENO" in "ENO FRUIT SALT",
/// not in "OMEPRAZOLE ENTERIC").
pub fn contains_phrase(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(at, _)| {
        let before = text[..at].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = text[at + phrase.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        before && after
    })
}

/// The shop's own name for `group`'s category, if it already has one
/// ("Pain relief" instead of "Painkiller").
pub fn shop_category<'a>(group: &Group, shop: &'a [String]) -> Option<&'a String> {
    let ours = group.category.to_uppercase();
    shop.iter().find(|s| {
        let upper = s.to_uppercase();
        upper == ours || upper.split(|c: char| !c.is_alphanumeric()).any(|w| !w.is_empty() && group.also.contains(&w))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_and_shop_names() {
        assert_eq!(group_of(&["PANADOL EXTRA"]).map(|g| g.category), Some("Painkiller"));
        assert_eq!(group_of(&["CHLORAMPHENICOL EYE DROPS"]).map(|g| g.category), Some("Eye & ear"));
        assert_eq!(group_of(&["TAKE WITH ENO"]).map(|g| g.category), Some("Stomach & digestion"));
        assert!(group_of(&["ENOUGH WATER"]).is_none(), "whole words only");
        assert_eq!(group_of(&["AMOXIL CAPSULES IF COUGH PERSISTS"]).map(|g| g.category), Some("Antibiotic"));
        assert_eq!(group_of(&["COUGH SYRUP"]).map(|g| g.category), Some("Cough & cold"));
        // The name's medicine beats another one named on the box.
        let combo = group_of(&["PIRITON", "DO NOT TAKE WITH PARACETAMOL"]);
        assert_eq!(combo.map(|g| g.category), Some("Allergy"));
        assert_eq!(group_of(&["CHLORAMPHENICOL", "EYE DROPS"]).map(|g| g.category), Some("Eye & ear"));
        let shop = vec!["Antibiotics".to_string(), "Pain relief".to_string()];
        let pain = GROUPS.iter().find(|g| g.category == "Painkiller").unwrap();
        assert_eq!(shop_category(pain, &shop).map(String::as_str), Some("Pain relief"));
        let arv = GROUPS.iter().find(|g| g.category.starts_with("Antiretroviral")).unwrap();
        assert_eq!(shop_category(arv, &shop), None);
        // No name is listed twice, and every name is upper case.
        let mut all: Vec<&str> = GROUPS.iter().flat_map(|g| g.names.iter().copied()).collect();
        assert!(all.iter().all(|n| *n == n.to_uppercase()));
        let count = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), count);
    }
}
